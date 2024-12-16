use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::config::CONFIG;
use crate::utils::error::{Result};
use crate::{log_info};
use tokio::fs;

use super::CacheState;

#[derive(Debug, Clone)]
pub struct DataUnit {
    pub cache_file: String,
    pub ranges: Vec<(u64, u64)>,
    pub size: Option<u64>,
}

impl DataUnit {
    pub fn new(cache_file: String) -> Self {
        Self {
            cache_file,
            ranges: Vec::new(),
            size: None,
        }
    }
    
    pub fn is_complete(&self) -> bool {
        if let Some(size) = self.size {
            if let Some((_, end)) = self.ranges.first() {
                return *end >= size - 1;
            }
        }
        false
    }
    
    pub fn needs_allocation(&self, size: u64) -> bool {
        if let Some(current_size) = self.size {
            size > current_size
        } else {
            true
        }
    }
    
    pub fn contains_range(&self, start: u64, end: u64) -> bool {
        for &(range_start, range_end) in &self.ranges {
            if range_start <= start && range_end >= end {
                log_info!("Cache", "找到完整缓存范围: {}-{} 在 {}-{} 中", start, end, range_start, range_end);
                return true;
            }
        }
        log_info!("Cache", "未找到完整缓存范围: {}-{}", start, end);
        false
    }
    
    pub fn get_cached_ranges(&self, start: u64, end: u64) -> Vec<(u64, u64)> {
        let mut cached_ranges = Vec::new();
        for &(range_start, range_end) in &self.ranges {
            if range_start <= end && range_end >= start {
                let overlap_start = start.max(range_start);
                let overlap_end = end.min(range_end);
                if overlap_start <= overlap_end {
                    cached_ranges.push((overlap_start, overlap_end));
                    log_info!("Cache", "找到部分缓存范围: {}-{}", overlap_start, overlap_end);
                }
            }
        }
        
        // 合并重叠的范围
        if !cached_ranges.is_empty() {
            cached_ranges.sort_by_key(|r| r.0);
            let mut merged = vec![cached_ranges[0]];
            for &(start, end) in &cached_ranges[1..] {
                let last = merged.last_mut().unwrap();
                if start <= last.1 + 1 {
                    last.1 = end.max(last.1);
                } else {
                    merged.push((start, end));
                }
            }
            cached_ranges = merged;
            log_info!("Cache", "合并后的缓存范围: {:?}", cached_ranges);
        } else {
            log_info!("Cache", "未找到任何缓存范围: {}-{}", start, end);
        }
        
        cached_ranges
    }
    
    pub fn is_fully_cached(&self, start: u64, end: u64) -> bool {
        self.contains_range(start, end)
    }
}

pub struct UnitPool {
    cache_dir: PathBuf,
    pub(crate) cache_map: Arc<RwLock<HashMap<String, DataUnit>>>,
}

impl UnitPool {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir: cache_dir.clone(),
            cache_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn new_data_unit(cache_file: &str) -> DataUnit {
        DataUnit::new(cache_file.to_string())
    }
    
    pub async fn get_data_unit(&self, url: &str) -> Result<Option<DataUnit>> {
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        let cache_state_path = CONFIG.get_cache_state(url)?;
        let cache_state_path_str = cache_state_path.to_string_lossy().into_owned();
        
        // 先从内存中获取
        let cache_map = self.cache_map.read().await;
        if let Some(unit) = cache_map.get(&cache_path_str) {
            log_info!("Cache", "从内存获取缓存单元: {} 范围: {:?}", url, unit.ranges);
            return Ok(Some(unit.clone()));
        }
        drop(cache_map);
        
        // 如果内存中没有，尝试从文件加载
        if let Some(state) = self.get_cache_state(url).await? {
            log_info!("Cache", "从文件加载缓存状态: {}", cache_state_path_str);
            let mut cache_map = self.cache_map.write().await;
            // 异步读取文件大小
            let file_size = tokio::fs::metadata(&state.cache_file.clone().unwrap())
                .await
                .ok()
                .map(|m| m.len());
            let data_unit = DataUnit {
                cache_file: cache_path_str.clone(),
                ranges: state.ranges,
                size: file_size,
            };
            
            // 检查缓存范围是否满足当前请求
            if let Ok((start, end)) = crate::utils::range::parse_range("bytes=0-0") { // 尝试解析一个范围，这里只是为了获取一个有效的 Range 值，实际使用时需要根据实际请求的 Range 来判断
                if data_unit.is_fully_cached(start, end) {
                    log_info!("Cache", "缓存范围满足当前请求，直接返回缓存单元, url: {}", url);
                    cache_map.insert(cache_path_str, data_unit.clone());
                    return Ok(Some(data_unit));
                }
            }
            
            // 如果缓存范围不满足当前请求，则不插入缓存map，继续执行后续的网络请求逻辑
            log_info!("Cache", "缓存范围不满足当前请求，继续执行网络请求逻辑, url: {}", url);
            return Ok(Some(data_unit)); // 返回数据单元，但不插入缓存map
        }
        
        log_info!("Cache", "未找到缓存单元: {}", cache_path_str);
        Ok(None)
    }
    
    pub async fn update_cache(&self, url: &str, range: &str) -> Result<()> {
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        
        // 确保文件存在
        if !cache_path.exists() {
            if let Some(parent) = cache_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::File::create(&cache_path).await?;
        }
        
        let (start, end) = crate::utils::range::parse_range(range)?;
        
        let mut cache_map = self.cache_map.write().await;
        let data_unit = cache_map.entry(cache_path_str.clone())
            .or_insert_with(|| DataUnit::new(cache_path_str.clone()));
        
        log_info!("Cache", "更新缓存范围前: {:?}", data_unit.ranges);
        data_unit.ranges.push((start, end));
        data_unit.ranges.sort_by_key(|r| r.0);
        
        // 合并重叠的范围
        let mut i = 0;
        while i < data_unit.ranges.len() - 1 {
            let current = data_unit.ranges[i];
            let next = data_unit.ranges[i + 1];
            
            if current.1 + 1 >= next.0 {
                data_unit.ranges[i] = (current.0, next.1.max(current.1));
                data_unit.ranges.remove(i + 1);
            } else {
                i += 1;
            }
        }
        log_info!("Cache", "更新缓存范围后: {:?}", data_unit.ranges);
        
        // 保存缓存状态到文件
        let state = CacheState {
            cache_file: Some(cache_path),
            ranges: data_unit.ranges.clone(),
        };
        self.update_cache_state(url, &state).await?;
        
        Ok(())
    }
    
    pub async fn ensure_file_size(&self, path: &str, size: u64) -> Result<()> {
        let path = PathBuf::from(path);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::File::create(&path).await?;
        }
        
        let metadata = fs::metadata(&path).await?;
        if metadata.len() < size {
            fs::File::open(&path).await?.set_len(size).await?;
        }
        
        Ok(())
    }
    
    pub async fn get_cache_state(&self, url: &str) -> Result<Option<CacheState>> {
        let state_path = CONFIG.get_cache_state(url)?;
        
        if let Ok(state_json) = fs::read_to_string(&state_path).await {
            match serde_json::from_str::<CacheState>(&state_json) {
                Ok(state) => {
                    log_info!("Cache", "读取缓存状态: {} 范围: {:?}", state_path.display(), state.ranges);
                    Ok(Some(state))
                },
                Err(e) => {
                    log_info!("Cache", "解析缓存状态失败: {} - {}", state_path.display(), e);
                    Ok(None)
                },
            }
        } else {
            log_info!("Cache", "缓存状态文件不存在: {}", state_path.display());
            Ok(None)
        }
    }
    
    pub async fn update_cache_state(&self, url: &str, state: &CacheState) -> Result<()> {
        let state_path = CONFIG.get_cache_state(url)?;
        let state_json = serde_json::to_string_pretty(state)?;
        
        // 确保目录存在
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        
        fs::write(&state_path, state_json).await?;
        log_info!("Cache", "保存缓存状态: {} 范围: {:?}", state_path.display(), state.ranges);
        
        Ok(())
    }
} 
