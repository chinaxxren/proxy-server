use std::path::PathBuf;
use crate::utils::error::Result;
use futures::Stream;
use bytes::Bytes;
use futures::stream::BoxStream;

mod disk;
mod compression;
mod manager;

pub use disk::DiskStorage;
pub use manager::{StorageManager, StorageManagerConfig};

/// 存储引擎特征，定义存储系统的核心功能
#[async_trait::async_trait]
pub trait StorageEngine: Send + Sync {
    /// 写入数据流
    async fn write_stream<S>(&self, key: &str, stream: S, range: (u64, u64)) -> Result<u64>
    where
        S: Stream<Item = Result<Bytes>> + Send + Unpin;
    
    /// 读取数据流
    async fn read_stream(&self, key: &str, range: (u64, u64)) -> Result<BoxStream<'static, Result<Bytes>>>;
    
    /// 检查数据是否存在
    async fn exists(&self, key: &str) -> Result<bool>;
    
    /// 获取数据大小
    async fn get_size(&self, key: &str) -> Result<u64>;
    
    /// 删除数据
    async fn delete(&self, key: &str) -> Result<()>;
}

/// 存储配置
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub root_path: PathBuf,
    pub chunk_size: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root_path: PathBuf::from("./cache"),
            chunk_size: 8192, // 8KB 默认块���小
        }
    }
} 