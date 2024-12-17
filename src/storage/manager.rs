use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::interval;
use crate::utils::error::Result;
use super::StorageEngine;
use bytes::Bytes;
use futures::Stream;
use futures::stream::BoxStream;

/// 存储元数据
#[derive(Debug, Clone)]
struct StorageMetadata {
    key: String,
    size: u64,
    last_accessed: SystemTime,
}

/// 存储管理器配置
#[derive(Debug, Clone)]
pub struct StorageManagerConfig {
    pub max_total_size: u64,           // 最大总存储空间
    pub max_file_size: u64,            // 单个文件最大大小
    pub expiration_time: Duration,      // 缓存过期时间
    pub cleanup_interval: Duration,     // 清理检查间隔
    pub max_concurrent_ops: usize,      // 最大并发操作数
}

impl Default for StorageManagerConfig {
    fn default() -> Self {
        Self {
            max_total_size: 10 * 1024 * 1024 * 1024, // 10GB
            max_file_size: 1024 * 1024 * 1024,       // 1GB
            expiration_time: Duration::from_secs(24 * 60 * 60), // 24小时
            cleanup_interval: Duration::from_secs(60 * 60),     // 1小时
            max_concurrent_ops: 100,
        }
    }
}

pub struct StorageManager<E: StorageEngine + Send + Sync + 'static> {
    engine: Arc<E>,
    metadata: Arc<RwLock<HashMap<String, StorageMetadata>>>,
    config: StorageManagerConfig,
    semaphore: Arc<Semaphore>,
    current_size: Arc<RwLock<u64>>,
}

impl<E: StorageEngine + Send + Sync + 'static> StorageManager<E> {
    pub fn new(engine: E, config: StorageManagerConfig) -> Self {
        let manager = Self {
            engine: Arc::new(engine),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_ops)),
            current_size: Arc::new(RwLock::new(0)),
            config,
        };
        
        // 启动后台清理任务
        manager.start_cleanup_task();
        manager
    }

    /// 写入数据流
    pub async fn write<S>(&self, key: &str, stream: S, range: (u64, u64)) -> Result<u64>
    where
        S: Stream<Item = Result<Bytes>> + Send + Unpin + 'static,
    {
        // 获取并发控制许可
        let _permit = self.semaphore.acquire().await?;

        // 检查空间限制
        let size = range.1 - range.0 + 1;
        if size > self.config.max_file_size {
            return Err(crate::utils::error::ProxyError::Cache("文件大小超出限制".to_string()));
        }

        // 确保总空间足够
        self.ensure_space(size).await?;

        // 写入数据
        let bytes_written = self.engine.write_stream(key, stream, range).await?;

        // 更新元数据
        let mut metadata = self.metadata.write().await;
        metadata.insert(key.to_string(), StorageMetadata {
            key: key.to_string(),
            size: bytes_written,
            last_accessed: SystemTime::now(),
        });

        // 更新当前使用的空间
        let mut current_size = self.current_size.write().await;
        *current_size += bytes_written;

        Ok(bytes_written)
    }

    /// 读取数据流
    pub async fn read(&self, key: &str, range: (u64, u64)) -> Result<BoxStream<'static, Result<Bytes>>> {
        // 获取并发控制许可
        let _permit = self.semaphore.acquire().await?;

        // 更新访问时间
        {
            let mut metadata = self.metadata.write().await;
            if let Some(meta) = metadata.get_mut(key) {
                meta.last_accessed = SystemTime::now();
            }
        }

        // 读取数据
        self.engine.read_stream(key, range).await
    }

    /// 确保有足够的存储空间
    async fn ensure_space(&self, required_size: u64) -> Result<()> {
        let current_size = self.current_size.write().await;
        if *current_size + required_size > self.config.max_total_size {
            // 清理过期和最少使用的数据
            self.cleanup_space(required_size).await?;
        }
        Ok(())
    }

    /// 清理空间
    async fn cleanup_space(&self, required_size: u64) -> Result<()> {
        let mut metadata = self.metadata.write().await;
        let mut current_size = self.current_size.write().await;
        
        // 按最后访问时间排序
        let mut entries: Vec<_> = metadata.values().cloned().collect();
        entries.sort_by(|a, b| a.last_accessed.cmp(&b.last_accessed));

        // 清理直到有足够空间
        for entry in entries {
            if *current_size + required_size <= self.config.max_total_size {
                break;
            }
            
            // 删除文件
            if let Ok(()) = self.engine.delete(&entry.key).await {
                *current_size -= entry.size;
                metadata.remove(&entry.key);
            }
        }

        Ok(())
    }

    /// 启动后台清理任务
    fn start_cleanup_task(&self) {
        let config = self.config.clone();
        let metadata = self.metadata.clone();
        let engine = self.engine.clone();
        let current_size = self.current_size.clone();

        tokio::spawn(async move {
            let mut interval = interval(config.cleanup_interval);
            loop {
                interval.tick().await;
                
                let mut metadata = metadata.write().await;
                let mut current_size = current_size.write().await;
                let now = SystemTime::now();

                // 清理过期数据
                let expired: Vec<_> = metadata
                    .values()
                    .filter(|m| {
                        m.last_accessed + config.expiration_time < now
                    })
                    .map(|m| m.key.clone())
                    .collect();

                for key in expired {
                    if let Ok(()) = engine.delete(&key).await {
                        if let Some(meta) = metadata.remove(&key) {
                            *current_size -= meta.size;
                        }
                    }
                }
            }
        });
    }
} 