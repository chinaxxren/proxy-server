use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use futures::Stream;
use bytes::Bytes;

use crate::utils::error::Result;
use super::StorageEngine;

#[derive(Clone)]
pub struct StorageManagerConfig {
    pub max_cache_size: u64,
    pub max_file_count: usize,
    pub cleanup_interval: Duration,
}

impl Default for StorageManagerConfig {
    fn default() -> Self {
        Self {
            max_cache_size: 1024 * 1024 * 1024, // 1GB
            max_file_count: 1000,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

#[derive(Clone)]
struct CacheEntry {
    key: String,
    total_size: u64,     // 文件的总大小
    last_access: SystemTime,
}

pub struct StorageManager<E> {
    engine: Arc<E>,
    config: StorageManagerConfig,
    cache_entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
    total_size: Arc<RwLock<u64>>,
}

impl<E: StorageEngine + 'static> StorageManager<E> {
    pub fn new(engine: E, config: StorageManagerConfig) -> Self {
        let manager = Self {
            engine: Arc::new(engine),
            config,
            cache_entries: Arc::new(RwLock::new(HashMap::new())),
            total_size: Arc::new(RwLock::new(0)),
        };
        
        // 启动清理任务
        manager.start_cleanup();
        manager
    }
    
    fn start_cleanup(&self) {
        let cache_entries = self.cache_entries.clone();
        let total_size = self.total_size.clone();
        let config = self.config.clone();
        let engine = self.engine.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(config.cleanup_interval).await;
                
                // 检查缓存大小
                let mut entries = cache_entries.write().await;
                let mut total = total_size.write().await;
                
                if *total <= config.max_cache_size && entries.len() <= config.max_file_count {
                    continue;
                }
                
                // 收集需要删除的条目
                let mut to_remove = Vec::new();
                {
                    // 按最后访问时间排序
                    let mut entry_list: Vec<_> = entries.values().cloned().collect();
                    entry_list.sort_by(|a, b| a.last_access.cmp(&b.last_access));
                    
                    // 收集要删除的键，直到满足大小限制
                    let mut current_total = *total;
                    let mut current_count = entries.len();
                    
                    for entry in entry_list {
                        if current_total <= config.max_cache_size && current_count <= config.max_file_count {
                            break;
                        }
                        current_total -= entry.total_size;
                        current_count -= 1;
                        to_remove.push(entry);
                    }
                }
                
                // 删除收集到的条目
                for entry in to_remove {
                    // 使用空流写入来清除文件
                    if let Ok(_) = engine.write(&entry.key, futures::stream::empty(), (0, 0)).await {
                        if let Some(removed) = entries.remove(&entry.key) {
                            *total -= removed.total_size;
                        }
                    }
                }
            }
        });
    }
    
    pub async fn write<S>(&self, key: &str, stream: S, range: (u64, u64)) -> Result<u64>
    where
        S: Stream<Item = Result<Bytes>> + Send + Unpin + 'static,
    {
        let bytes_written = self.engine.write(key, stream, range).await?;
        
        // 更新缓存信息
        let mut entries = self.cache_entries.write().await;
        let mut total = self.total_size.write().await;
        
        let end_pos = range.0 + bytes_written;
        
        if let Some(entry) = entries.get_mut(key) {
            // 更新文件的总大小（如果新写入的范围扩展了文件）
            if end_pos > entry.total_size {
                *total = *total - entry.total_size + end_pos;
                entry.total_size = end_pos;
            }
            entry.last_access = SystemTime::now();
        } else {
            entries.insert(key.to_string(), CacheEntry {
                key: key.to_string(),
                total_size: end_pos,
                last_access: SystemTime::now(),
            });
            *total += end_pos;
        }
        
        Ok(bytes_written)
    }
    
    pub async fn read(&self, key: &str, range: (u64, u64)) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>> {
        // 更新访问时间
        if let Some(entry) = self.cache_entries.write().await.get_mut(key) {
            entry.last_access = SystemTime::now();
        }
        
        // 读取数据
        self.engine.read(key, range).await
    }

    pub async fn get_size(&self, key: &str) -> Result<Option<u64>> {
        // 从缓存条目中获取大小
        if let Some(entry) = self.cache_entries.read().await.get(key) {
            return Ok(Some(entry.total_size));
        }
        
        // 如果缓存中没有，从存储引擎获取
        self.engine.get_size(key).await
    }

    pub async fn check_range(&self, key: &str, range: (u64, u64)) -> Result<bool> {
        // 从缓存条目中检查范围
        if let Some(entry) = self.cache_entries.read().await.get(key) {
            // 检查请求的范围是否在缓存的文件大小范围内
            if range.0 >= entry.total_size {
                return Ok(false);
            }
            
            let end = if range.1 == u64::MAX {
                entry.total_size - 1
            } else {
                range.1
            };
            
            return Ok(end < entry.total_size);
        }
        
        // 如果缓存中没有，从存储引擎检查
        self.engine.check_range(key, range).await
    }
} 