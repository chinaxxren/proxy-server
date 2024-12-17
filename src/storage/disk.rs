use std::path::PathBuf;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use futures::{Stream, StreamExt};
use bytes::Bytes;
use async_stream::try_stream;
use futures::stream::BoxStream;

use super::{StorageEngine, StorageConfig};
use crate::utils::error::{Result, ProxyError};

/// 磁盘存储引擎
pub struct DiskStorage {
    config: StorageConfig,
}

impl DiskStorage {
    pub fn new(config: StorageConfig) -> Self {
        Self { config }
    }

    /// 获取文件路径
    fn get_file_path(&self, key: &str) -> PathBuf {
        self.config.root_path.join(key)
    }

    /// 确保目录存在
    async fn ensure_dir(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !tokio::fs::try_exists(parent).await? {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl StorageEngine for DiskStorage {
    async fn write_stream<S>(&self, key: &str, mut stream: S, range: (u64, u64)) -> Result<u64>
    where
        S: Stream<Item = Result<Bytes>> + Send + Unpin,
    {
        let path = self.get_file_path(key);
        self.ensure_dir(&path).await?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .await?;

        // 定位到写入位置
        file.seek(SeekFrom::Start(range.0)).await?;

        let mut bytes_written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            bytes_written += chunk.len() as u64;
        }

        // 确保数据写入磁盘
        file.flush().await?;
        
        Ok(bytes_written)
    }

    async fn read_stream(&self, key: &str, range: (u64, u64)) -> Result<BoxStream<'static, Result<Bytes>>> {
        let path = self.get_file_path(key);
        
        if !tokio::fs::try_exists(&path).await? {
            return Err(ProxyError::Cache(format!("文件不存在: {}", key)));
        }

        let mut file = File::open(&path).await?;
        let file_size = file.metadata().await?.len();

        // 验证范围
        let start = range.0;
        let end = if range.1 == 0 { file_size - 1 } else { range.1.min(file_size - 1) };
        
        if start >= file_size {
            return Err(ProxyError::Cache("请求范围超出文件大小".to_string()));
        }

        // 定位到起始位置
        file.seek(SeekFrom::Start(start)).await?;

        let chunk_size = self.config.chunk_size;
        let remaining = end - start + 1;

        let stream = try_stream! {
            let mut pos = 0u64;
            while pos < remaining {
                let chunk_size = chunk_size.min((remaining - pos) as usize);
                let mut buffer = vec![0; chunk_size];
                let n = file.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                buffer.truncate(n);
                pos += n as u64;
                yield Bytes::from(buffer);
            }
        };

        Ok(Box::pin(stream))
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let path = self.get_file_path(key);
        Ok(tokio::fs::try_exists(&path).await?)
    }

    async fn get_size(&self, key: &str) -> Result<u64> {
        let path = self.get_file_path(key);
        if !tokio::fs::try_exists(&path).await? {
            return Ok(0);
        }
        let metadata = tokio::fs::metadata(&path).await?;
        Ok(metadata.len())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.get_file_path(key);
        if tokio::fs::try_exists(&path).await? {
            tokio::fs::remove_file(&path).await?;
        }
        Ok(())
    }
} 