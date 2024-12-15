use crate::cache::unit_pool::UnitPool;
use crate::utils::error::{ProxyError, Result};
use crate::config::CONFIG;
use crate::{log_info, log_error};
use std::sync::Arc;
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

    pub async fn merge_files_if_needed(&self, url: &str) -> Result<()> {
        let cache = self.unit_pool.cache_map.read().await;
        if let Some(ranges) = cache.get(url) {
            let merged = merge_ranges(ranges.clone());
            drop(cache); // 释放锁
            let mut cache = self.unit_pool.cache_map.write().await;
            if let Some(entry) = cache.get_mut(url) {
                *entry = merged;
            }
        }
        Ok(())
    }

    pub async fn clean_cache(&self) -> Result<()> {
        log_info!("Cache", "开始清理缓存...");
        let mut total_size = 0;
        let mut cleaned_count = 0;
        
        let cache = self.unit_pool.cache_map.read().await;
        let urls: Vec<String> = cache.keys().cloned().collect();
        drop(cache);
        
        for url in urls {
            let file_path = CONFIG.get_cache_path(&url);
            if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                let file_size = metadata.len();
                total_size += file_size;
                
                // 检查文件大小（超过100MB的文件）
                if file_size > 1024 * 1024 * 100 {
                    if let Err(e) = tokio::fs::remove_file(&file_path).await {
                        log_error!("Cache", "删除文件失败 {}: {}", file_path.display(), e);
                        continue;
                    }
                    let mut cache = self.unit_pool.cache_map.write().await;
                    cache.remove(&url);
                    cleaned_count += 1;
                    log_info!("Cache", "删除大文件: {} ({} MB)", url, file_size / 1024 / 1024);
                    continue;
                }
                
                // 检查文件访问时间（超过24小时的文件）
                if let Ok(time) = metadata.modified() {
                    if time.elapsed().unwrap().as_secs() > 24 * 60 * 60 {
                        if let Err(e) = tokio::fs::remove_file(&file_path).await {
                            log_error!("Cache", "删除文件失败 {}: {}", file_path.display(), e);
                            continue;
                        }
                        let mut cache = self.unit_pool.cache_map.write().await;
                        cache.remove(&url);
                        cleaned_count += 1;
                        log_info!("Cache", "删除过期文件: {}", url);
                    }
                }
            }
        }
        
        // 保存更新后的缓存状态
        self.unit_pool.save_cache_state().await?;
        
        log_info!(
            "Cache", 
            "缓存清理完成: 清理了 {} 个文件, 当前缓存总大小: {} MB", 
            cleaned_count,
            total_size / 1024 / 1024
        );
        
        Ok(())
    }

    pub async fn validate_cache(&self, url: &str) -> Result<()> {
        let cache = self.unit_pool.cache_map.read().await;
        if let Some(ranges) = cache.get(url) {
            for &(start, end) in ranges.iter() {
                let file_path = CONFIG.get_cache_path(url);
                // 添加文件完整性校验
                let metadata = tokio::fs::metadata(&file_path).await?;
                if metadata.len() < end + 1 {
                    return Err(ProxyError::Cache("缓存文件不完整".to_string()));
                }
                // 添加数据校验和验证
                if !self.verify_data_checksum(&file_path, start, end).await? {
                    return Err(ProxyError::Cache("缓存数据校验失败".to_string()));
                }
            }
        }
        Ok(())
    }

    async fn verify_data_checksum(&self, path: &std::path::Path, start: u64, end: u64) -> Result<bool> {
        let mut file = tokio::fs::File::open(path).await?;
        file.seek(SeekFrom::Start(start)).await?;
        
        let mut buffer = vec![0; (end - start + 1) as usize];
        file.read_exact(&mut buffer).await?;

        // 检查数据有效性
        if buffer.is_empty() {
            return Err(ProxyError::Cache("缓存数据为空".to_string()));
        }

        // 检查数据完整性
        if buffer.len() as u64 != (end - start + 1) {
            return Err(ProxyError::Cache("缓存数据长度不匹配".to_string()));
        }

        Ok(true)
    }
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
