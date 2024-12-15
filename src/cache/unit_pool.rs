use crate::config::CONFIG;
use crate::utils::parse_range;
use crate::utils::error::Result;
use std::collections::HashMap;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::io::SeekFrom;
use std::path::Path;
use serde_json;

#[derive(Clone)]
pub struct UnitPool {
    // 维护缓存单元的状态
    pub cache_map: Arc<Mutex<HashMap<String, Vec<(u64, u64)>>>>,
}

impl UnitPool {
    pub fn new() -> Self {
        Self {
            cache_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn is_fully_cached(&self, url: &str, range: &str) -> Result<bool> {
        // 检查指定 URL 的缓存是否完整
        let cache = self.cache_map.lock().await;
        if let Some(ranges) = cache.get(url) {
            // 解析 Range，例如 "bytes=0-1023"
            let requested = parse_range(range)?;
            // 检查是否有一个缓存区间完全覆盖请求区间
            for &(cached_start, cached_end) in ranges.iter() {
                if cached_start <= requested.0 && cached_end >= requested.1 {
                    return Ok(true);
                }
            }
            Ok(false)
        } else {
            Ok(false)
        }
    }

    pub async fn is_partially_cached(&self, url: &str, range: &str) -> Result<bool> {
        // 检查指定 URL 的缓存是否部分存在
        let cache = self.cache_map.lock().await;
        if let Some(ranges) = cache.get(url) {
            let requested = parse_range(range)?;
            // 检查否有任何缓存区间与请求区间重叠
            for &(cached_start, cached_end) in ranges.iter() {
                if cached_start <= requested.1 && cached_end >= requested.0 {
                    return Ok(true);
                }
            }
            Ok(false)
        } else {
            Ok(false)
        }
    }

    pub async fn update_cache(&self, url: &str, range: &str) -> Result<()> {
        // 更新缓存区间信息（添加新的缓存区间）
        let mut cache = self.cache_map.lock().await;
        let ranges = cache.entry(url.to_string()).or_insert(Vec::new());
        let new_range = parse_range(range)?;
        ranges.push(new_range);
        // 合并重叠或相邻的区间
        *ranges = merge_ranges(ranges.clone());
        Ok(())
    }

    pub async fn write_cache(&self, url: &str, range: &str, data: &[u8]) -> Result<(u64, u64)> {
        let file_path = CONFIG.get_cache_path(url);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&file_path)
            .await?;

        let requested = parse_range(range)?;
        file.seek(SeekFrom::Start(requested.0)).await?;
        file.write_all(data).await?;

        self.update_cache(url, range).await?;
        
        Ok(requested)
    }

    pub async fn merge_range(&self, url: &str, ranges: &[(u64, u64)]) -> Result<()> {
        let mut cache = self.cache_map.lock().await;
        if let Some(existing_ranges) = cache.get_mut(url) {
            existing_ranges.extend_from_slice(ranges);
            *existing_ranges = merge_ranges(existing_ranges.clone());
        }
        Ok(())
    }

    pub async fn optimize_cache_ranges(&self, url: &str) -> Result<()> {
        let mut cache = self.cache_map.lock().await;
        if let Some(ranges) = cache.get_mut(url) {
            // 合并过小的区间
            let min_size = 1024 * 64; // 64KB
            let mut optimized = Vec::new();
            let mut current = ranges[0];
            
            for &(start, end) in ranges.iter().skip(1) {
                if start - current.1 <= min_size as u64 {
                    current.1 = end;
                } else {
                    optimized.push(current);
                    current = (start, end);
                }
            }
            optimized.push(current);
            *ranges = optimized;
        }
        Ok(())
    }

    pub async fn write_data(&self, path: &Path, data: &[u8]) -> Result<()> {
        let mut file = tokio::fs::File::create(path).await?;
        file.write_all(data).await?;
        Ok(())
    }

    pub async fn get_cached_ranges(&self, url: &str) -> Result<Vec<(u64, u64)>> {
        let cache = self.cache_map.lock().await;
        Ok(cache.get(url).cloned().unwrap_or_default())
    }

    pub async fn update_cached_ranges(&self, url: &str, ranges: Vec<(u64, u64)>) -> Result<()> {
        let mut cache = self.cache_map.lock().await;
        if ranges.is_empty() {
            cache.remove(url);
        } else {
            cache.insert(url.to_string(), ranges);
        }
        Ok(())
    }

    pub async fn save_cache_state(&self) -> Result<()> {
        let cache = self.cache_map.lock().await;
        let cache_path = CONFIG.get_cache_path("cache_state");
        let cache_data = serde_json::to_string(&*cache)?;
        tokio::fs::write(cache_path, cache_data).await?;
        Ok(())
    }

    pub async fn load_cache_state(&self) -> Result<()> {
        let cache_path = CONFIG.get_cache_path("cache_state");
        if let Ok(data) = tokio::fs::read_to_string(cache_path).await {
            if let Ok(state) = serde_json::from_str(&data) {
                let mut cache = self.cache_map.lock().await;
                *cache = state;
            }
        }
        Ok(())
    }

    // 其他缓存管理方法
}

fn merge_ranges(mut ranges: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|k| k.0);
    let mut merged = Vec::new();
    let mut current = ranges[0];
    for &(start, end) in ranges.iter().skip(1) {
        if start <= current.1 + 1 {
            current.1 = current.1.max(end);
        } else {
            merged.push(current);
            current = (start, end);
        }
    }
    merged.push(current);
    merged
}
