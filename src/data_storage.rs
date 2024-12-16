use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::cache::unit_pool::DataUnit;
use crate::config::CONFIG;
use crate::utils::error::Result;
use crate::{log_info};
use tokio::fs;

pub struct DataStorage {
    cache_map: Arc<RwLock<HashMap<String, DataUnit>>>,
}

impl DataStorage {
    pub fn new() -> Self {
        Self {
            cache_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn check_cache(&self, url: &str) -> Result<bool> {
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        
        log_info!("Storage", "检查缓存文件: {}", url);
        
        if !fs::try_exists(&cache_path).await? {
            log_info!("Storage", "缓存文件不存在: {}", url);
            return Ok(false);
        }
        
        let cache_map = self.cache_map.read().await;
        if let Some(data_unit) = cache_map.get(&cache_path_str) {
            return Ok(data_unit.is_complete());
        }
        
        Ok(false)
    }
    
    pub async fn get_cache_size(&self, url: &str) -> Result<u64> {
        let cache_file = CONFIG.get_cache_file(url)?;
        let cache_file_str = cache_file.to_string_lossy().into_owned();
        
        log_info!("Storage", "检查缓存文件: {}", url);
        
        if !fs::try_exists(&cache_file).await? {
            log_info!("Storage", "缓存文件不存在: {}", url);
            return Ok(0);
        }
        
        let metadata = fs::metadata(&cache_file).await?;
        
        if metadata.len() == 0 {
            log_info!("Storage", "缓存文件大小为0，删除文件: {}", url);
            fs::remove_file(&cache_file).await?;
            return Ok(0);
        }
        
        Ok(metadata.len())
    }
    
    pub async fn optimize_cache(&self, url: &str) -> Result<()> {
        let cache_file = CONFIG.get_cache_file(url)?;
        let cache_file_str = cache_file.to_string_lossy().into_owned();
        
        log_info!("Storage", "检查缓存文件: {}", url);
        
        if !fs::try_exists(&cache_file).await? {
            log_info!("Storage", "缓存文件不存在，跳过优化: {}", url);
            return Ok(());
        }
        
        let metadata = fs::metadata(&cache_file).await?;
        
        if metadata.len() == 0 {
            log_info!("Storage", "缓存文件大小为0，删除文件: {}", url);
            fs::remove_file(&cache_file).await?;
        }
        
        Ok(())
    }
    
    pub async fn get_data_unit(&self, url: &str) -> Result<Option<DataUnit>> {
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        
        let mut cache_map = self.cache_map.write().await;
        if let Some(data_unit) = cache_map.get_mut(&cache_path_str) {
            return Ok(Some(data_unit.clone()));
        }
        
        Ok(None)
    }
}