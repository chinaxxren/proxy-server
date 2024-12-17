use std::path::PathBuf;
use futures::Stream;
use bytes::Bytes;
use crate::utils::error::Result;

pub mod block;
pub mod disk;
pub mod manager;

pub use disk::DiskStorage;
pub use manager::{StorageManager, StorageManagerConfig};

#[derive(Clone)]
pub struct StorageConfig {
    pub root_path: PathBuf,
    pub chunk_size: usize,
}

#[async_trait::async_trait]
pub trait StorageEngine: Send + Sync {
    async fn write<S>(&self, key: &str, stream: S, range: (u64, u64)) -> Result<u64>
    where
        S: Stream<Item = Result<Bytes>> + Send + Unpin + 'static;

    async fn read(&self, key: &str, range: (u64, u64)) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>>;

    async fn get_size(&self, key: &str) -> Result<Option<u64>>;

    async fn check_range(&self, key: &str, range: (u64, u64)) -> Result<bool>;
} 