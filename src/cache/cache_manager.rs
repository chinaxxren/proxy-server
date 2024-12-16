use std::sync::Arc;
use crate::utils::error::{Result, ProxyError};
use crate::config::CONFIG;
use crate::{log_error, log_info};
use crate::cache::unit_pool::UnitPool;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;

#[derive(Clone)]
pub struct CacheManager {
    unit_pool: Arc<UnitPool>,
}

impl CacheManager {
    pub fn new(unit_pool: Arc<UnitPool>) -> Self {
        Self { unit_pool }
    }

    pub async fn clean_cache(&self) -> Result<()> {
        log_info!("Cache", "开始清理缓存...");
        let mut total_size = 0;
        let mut cleaned_count = 0;
        
        // 获取所有缓存的文件路径
        let cache = self.unit_pool.cache_map.read().await;
        let cache_paths: Vec<String> = cache.keys().cloned().collect();
        drop(cache);
        
        for cache_path in cache_paths {
            if let Ok(metadata) = tokio::fs::metadata(&cache_path).await {
                let file_size = metadata.len();
                total_size += file_size;
                
                // 检查文件大小（超过100MB的文件）
                if file_size > 1024 * 1024 * 100 {
                    if let Err(e) = tokio::fs::remove_file(&cache_path).await {
                        log_error!("Cache", "删除文件失败 {}: {}", cache_path, e);
                        continue;
                    }
                    
                    // 删除对应的状态文件
                    let state_path = CONFIG.get_cache_state(&cache_path);
                    if let Err(e) = tokio::fs::remove_file(&state_path).await {
                        log_error!("Cache", "删除状态文件失败 {}: {}", state_path, e);
                    }
                    
                    let mut cache = self.unit_pool.cache_map.write().await;
                    cache.remove(&cache_path);
                    cleaned_count += 1;
                    log_info!("Cache", "删除大文件: {} ({} MB)", cache_path, file_size / 1024 / 1024);
                }
            }
        }
        
        // 清理过期缓存
        self.unit_pool.clean_old_cache(24).await?; // 24小时过期
        
        log_info!(
            "Cache", 
            "缓存清理完成: 清理了 {} 个文件, 当前缓存总大小: {} MB", 
            cleaned_count,
            total_size / 1024 / 1024
        );
        
        Ok(())
    }

    pub async fn validate_cache(&self, url: &str) -> Result<()> {
        let cache_path = CONFIG.get_cache_file(url);
        let cache = self.unit_pool.cache_map.read().await;
        if let Some(data_unit) = cache.get(&cache_path) {
            let cache_file = &data_unit.cache_file;
            
            // 检查文件是否存在
            if !tokio::fs::try_exists(cache_file).await? {
                return Err(ProxyError::Cache("缓存文件不存在".to_string()));
            }

            // 检查文件大小
            let metadata = tokio::fs::metadata(cache_file).await?;
            let file_size = metadata.len();

            // 检查每个缓存区间
            for &(start, end) in &data_unit.ranges {
                // 检查区间是否超出文件大小
                if end >= file_size {
                    return Err(ProxyError::Cache("缓存文件不完整".to_string()));
                }

                // 验证区间数据
                if !self.verify_range_data(cache_file, start, end).await? {
                    return Err(ProxyError::Cache("缓存数据校验失败".to_string()));
                }
            }
        }
        Ok(())
    }

    async fn verify_range_data(&self, path: &str, start: u64, end: u64) -> Result<bool> {
        let mut file = tokio::fs::File::open(path).await?;
        file.seek(SeekFrom::Start(start)).await?;
        
        let mut buffer = vec![0; (end - start + 1) as usize];
        if let Err(e) = file.read_exact(&mut buffer).await {
            log_error!("Cache", "读取缓存数据失败: {}", e);
            return Ok(false);
        }

        // 这里可以添加数据完整性校验，比如校验和
        Ok(true)
    }

    pub async fn optimize_cache(&self, url: &str) -> Result<()> {
        let cache_path = CONFIG.get_cache_file(url);
        let mut cache = self.unit_pool.cache_map.write().await;
        if let Some(data_unit) = cache.get_mut(&cache_path) {
            // 获取所有区间
            let ranges = data_unit.ranges.clone();
            
            // 清空现有区间
            data_unit.ranges.clear();
            
            // 重新添加并合并区间
            for (start, end) in ranges {
                data_unit.add_range(start, end);
            }
            
            // 保存更新后的状态
            drop(cache);
            self.unit_pool.save_cache_state(url).await?;
        }
        Ok(())
    }
} 