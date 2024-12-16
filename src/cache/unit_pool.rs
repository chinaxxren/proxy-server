use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::config::CONFIG;
use crate::utils::error::{Result, ProxyError};
use crate::{log_info, log_error};
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
}

pub struct UnitPool {
    cache_dir: PathBuf,
    pub(crate) cache_map: Arc<RwLock<HashMap<String, DataUnit>>>,
}

impl UnitPool {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            cache_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn new_data_unit(cache_file: &str) -> DataUnit {
        DataUnit::new(cache_file.to_string())
    }
    
    pub async fn get_data_unit(&self, url: &str) -> Result<Option<DataUnit>> {
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        
        let cache_map = self.cache_map.read().await;
        Ok(cache_map.get(&cache_path_str).cloned())
    }
    
    pub async fn update_cache(&self, url: &str, range: &str) -> Result<()> {
        let (start, end) = crate::utils::range::parse_range(range)?;
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        
        let mut cache_map = self.cache_map.write().await;
        if let Some(data_unit) = cache_map.get_mut(&cache_path_str) {
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
        }
        
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
                Ok(state) => Ok(Some(state)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
    
    pub async fn update_cache_state(&self, url: &str, state: &CacheState) -> Result<()> {
        let state_path = CONFIG.get_cache_state(url)?;
        let state_json = serde_json::to_string_pretty(state)?;
        fs::write(&state_path, state_json).await?;
        Ok(())
    }
} 
