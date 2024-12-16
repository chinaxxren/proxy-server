use std::collections::HashMap;
use tokio::sync::RwLock;
use crate::utils::error::Result;
use crate::cache::data_unit::DataUnit;
use crate::config::CONFIG;
use crate::{log_info, log_error};
use tokio::fs::File;


pub struct UnitPool {
    pub cache_map: RwLock<HashMap<String, DataUnit>>,
}

impl UnitPool {
    pub fn new() -> Self {
        Self {
            cache_map: RwLock::new(HashMap::new()),
        }
    }

    pub async fn update_cache(&self, url: &str, range: &str) -> Result<()> {
        let (start, end) = crate::utils::range::parse_range(range)?;
        let cache_path = CONFIG.get_cache_file(url);
        
        let mut cache_map = self.cache_map.write().await;
        let data_unit = cache_map.entry(cache_path.clone()).or_insert_with(|| {
            DataUnit::new(cache_path.clone())
        });
        
        // 检查是否需要扩展文件大小
        if data_unit.needs_allocation(end + 1) {
            self.ensure_file_size(&data_unit.cache_file, end + 1).await?;
            data_unit.update_allocated_size(end + 1);
            
            // 更新文件总大小
            if let Ok(metadata) = tokio::fs::metadata(&data_unit.cache_file).await {
                let file_size = metadata.len();
                if data_unit.total_size.is_none() || data_unit.total_size.unwrap() < file_size {
                    data_unit.set_total_size(file_size);
                }
            }
        }
        
        data_unit.add_range(start, end);
        
        // 保存缓存状态
        drop(cache_map);
        self.save_cache_state(url).await?;
        
        log_info!("Cache", "更新缓存: {} {}-{}", url, start, end);
        Ok(())
    }

    async fn ensure_file_size(&self, path: &str, size: u64) -> Result<()> {
        let file = File::options()
            .create(true)
            .write(true)
            .open(path)
            .await?;

        file.set_len(size).await?;
        Ok(())
    }

    pub async fn is_fully_cached(&self, url: &str, range: &str) -> Result<bool> {
        let cache_path = CONFIG.get_cache_file(url);
        let cache_map = self.cache_map.read().await;
        
        if let Some(data_unit) = cache_map.get(&cache_path) {
            if let Ok((start, end)) = crate::utils::range::parse_range(range) {
                return Ok(data_unit.contains_range(start, end));
            }
        }
        
        Ok(false)
    }

    pub async fn is_partially_cached(&self, url: &str, range: &str) -> Result<bool> {
        let cache_path = CONFIG.get_cache_file(url);
        let cache_map = self.cache_map.read().await;
        
        if let Some(data_unit) = cache_map.get(&cache_path) {
            if let Ok((start, end)) = crate::utils::range::parse_range(range) {
                return Ok(data_unit.partially_contains_range(start, end));
            }
        }
        
        Ok(false)
    }

    pub async fn get_cached_ranges(&self, url: &str) -> Result<Vec<(u64, u64)>> {
        let cache_path = CONFIG.get_cache_file(url);
        let cache_map = self.cache_map.read().await;
        
        if let Some(data_unit) = cache_map.get(&cache_path) {
            Ok(data_unit.ranges.clone())
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn get_cache_file(&self, url: &str) -> Result<Option<String>> {
        let cache_path = CONFIG.get_cache_file(url);
        let cache_map = self.cache_map.read().await;
        Ok(cache_map.get(&cache_path).map(|unit| unit.cache_file.clone()))
    }

    pub async fn set_total_size(&self, url: &str, size: u64) -> Result<()> {
        let cache_path = CONFIG.get_cache_file(url);
        let mut cache_map = self.cache_map.write().await;
        
        if let Some(data_unit) = cache_map.get_mut(&cache_path) {
            data_unit.set_total_size(size);
            // 如果需要，预分配文件大小
            if data_unit.needs_allocation(size) {
                self.ensure_file_size(&data_unit.cache_file, size).await?;
                data_unit.update_allocated_size(size);
            }
            drop(cache_map);
            self.save_cache_state(url).await?;
            Ok(())
        } else {
            let mut data_unit = DataUnit::new(cache_path.clone());
            data_unit.set_total_size(size);
            // 预分配文件大小
            self.ensure_file_size(&data_unit.cache_file, size).await?;
            data_unit.update_allocated_size(size);
            cache_map.insert(cache_path, data_unit);
            drop(cache_map);
            self.save_cache_state(url).await?;
            Ok(())
        }
    }

    pub async fn clean_old_cache(&self, max_age_hours: i64) -> Result<()> {
        let mut cache_map = self.cache_map.write().await;
        let now = chrono::Utc::now();
        
        let mut to_remove = Vec::new();
        for (cache_path, data_unit) in cache_map.iter() {
            let age = now.signed_duration_since(data_unit.last_accessed);
            if age.num_hours() >= max_age_hours {
                to_remove.push(cache_path.clone());
                // 删除缓存文件
                if let Err(e) = tokio::fs::remove_file(&data_unit.cache_file).await {
                    log_error!("Cache", "删除缓存文件失败: {} - {}", data_unit.cache_file, e);
                }
                // 删除状态文件
                let state_path = CONFIG.get_cache_state(cache_path);
                if let Err(e) = tokio::fs::remove_file(&state_path).await {
                    log_error!("Cache", "删除状态文件失败: {} - {}", state_path, e);
                }
            }
        }
        
        // 从内存中移除
        for path in to_remove {
            cache_map.remove(&path);
        }
        
        Ok(())
    }

    // 保存单个 URL 的缓存状态
    pub async fn save_cache_state(&self, url: &str) -> Result<()> {
        let cache_path = CONFIG.get_cache_file(url);
        let state_path = CONFIG.get_cache_state(url);
        
        let cache_map = self.cache_map.read().await;
        if let Some(data_unit) = cache_map.get(&cache_path) {
            // 确保目录存在
            if let Some(parent) = std::path::Path::new(&state_path).parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            
            // 序列化并保存
            let state_json = serde_json::to_string_pretty(data_unit)?;
            tokio::fs::write(&state_path, state_json).await?;
            log_info!("Cache", "保存缓存状态: {} -> {:?}", url, data_unit);
        }
        
        Ok(())
    }

    // 加载单个 URL 的缓存状态
    pub async fn load_cache_state(&self, url: &str) -> Result<()> {
        let cache_path = CONFIG.get_cache_file(url);
        let state_path = CONFIG.get_cache_state(url);
        
        if let Ok(state_json) = tokio::fs::read_to_string(&state_path).await {
            if let Ok(mut data_unit) = serde_json::from_str::<DataUnit>(&state_json) {
                // 确保 total_size 不为 None
                if data_unit.total_size.is_none() {
                    if let Ok(metadata) = tokio::fs::metadata(&cache_path).await {
                        data_unit.set_total_size(metadata.len());
                    }
                }
                
                let mut cache_map = self.cache_map.write().await;
                cache_map.insert(cache_path, data_unit);
                log_info!("Cache", "加载缓存状态: {}", url);
            }
        }
        
        Ok(())
    }
}

impl Default for UnitPool {
    fn default() -> Self {
        Self::new()
    }
} 