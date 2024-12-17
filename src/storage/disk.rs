use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use tokio::fs as tokio_fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use futures::Stream;
use async_trait::async_trait;
use bytes::Bytes;
use md5;

use crate::utils::error::{Result, ProxyError};
use crate::log_info;
use super::{StorageEngine, StorageConfig};

pub struct DiskStorage {
    config: StorageConfig,
}

impl DiskStorage {
    pub fn new(config: StorageConfig) -> Self {
        Self { config }
    }

    fn get_file_path(&self, key: &str) -> PathBuf {
        // 使用MD5生成URL的哈希值
        let hash = format!("{:x}", md5::compute(key.as_bytes()));
        
        // 创建二级目录结构，使用哈希的前两个字符
        let dir1 = &hash[0..2];
        let dir2 = &hash[2..4];
        
        // 构建完整的文件路径
        self.config.root_path
            .join(dir1)
            .join(dir2)
            .join(hash)
    }

    async fn ensure_dir_exists(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                tokio_fs::create_dir_all(parent).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageEngine for DiskStorage {
    async fn write<S>(&self, key: &str, mut stream: S, range: (u64, u64)) -> Result<u64>
    where
        S: Stream<Item = Result<Bytes>> + Send + Unpin + 'static,
    {
        let file_path = self.get_file_path(key);
        self.ensure_dir_exists(&file_path).await?;

        log_info!("Storage", "写入文件: {:?}, 范围: {}-{}", file_path, range.0, range.1);
        
        let mut file = if file_path.exists() {
            tokio_fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&file_path)
                .await?
        } else {
            tokio_fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&file_path)
                .await?
        };

        // 设置文件写入位置
        file.seek(SeekFrom::Start(range.0)).await?;

        let mut written = 0u64;
        while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;
        }

        file.flush().await?;
        log_info!("Storage", "写入完成: {:?}, 写入字节数: {}", file_path, written);
        
        Ok(written)
    }

    async fn read(&self, key: &str, range: (u64, u64)) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>> {
        let file_path = self.get_file_path(key);
        
        if !file_path.exists() {
            return Err(ProxyError::Storage(format!("文件不存在: {:?}", file_path)));
        }

        log_info!("Storage", "读取文件: {:?}, 范围: {}-{}", file_path, range.0, range.1);
        
        let file = File::open(&file_path)?;
        let metadata = file.metadata()?;
        let file_size = metadata.len();
        
        if range.0 >= file_size {
            return Err(ProxyError::Storage("请求范围超出文件大小".to_string()));
        }

        // 计算实际的结束位置
        let end = if range.1 == u64::MAX {
            file_size - 1
        } else {
            std::cmp::min(range.1, file_size - 1)
        };

        // 计算需要读取的总字节数
        let total_bytes = end - range.0 + 1;
        log_info!("Storage", "需要读取的总字节数: {} (范围: {}-{})", total_bytes, range.0, end);

        let chunk_size = self.config.chunk_size;
        
        // 创建异步读取流
        let stream = Box::pin(futures::stream::try_unfold(
            (file, range.0, end, chunk_size, 0u64, total_bytes),
            |(mut file, start, end, chunk_size, mut bytes_read, total_bytes)| async move {
                if bytes_read >= total_bytes {
                    return Ok(None);
                }

                let remaining = total_bytes - bytes_read;
                let to_read = std::cmp::min(chunk_size as u64, remaining) as usize;
                let mut buffer = vec![0; to_read];

                file.seek(SeekFrom::Start(start + bytes_read))?;
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    return Ok(None);
                }

                buffer.truncate(n);
                bytes_read += n as u64;

                log_info!("Storage", "读取数据块: {} 字节, 已读取: {}/{} 字节", 
                    n, bytes_read, total_bytes);

                Ok(Some((Bytes::from(buffer), (file, start, end, chunk_size, bytes_read, total_bytes))))
            },
        ));

        Ok(Box::new(stream))
    }

    async fn get_size(&self, key: &str) -> Result<Option<u64>> {
        let file_path = self.get_file_path(key);
        if !file_path.exists() {
            return Ok(None);
        }

        let metadata = tokio_fs::metadata(&file_path).await?;
        Ok(Some(metadata.len()))
    }

    async fn check_range(&self, key: &str, range: (u64, u64)) -> Result<bool> {
        let file_path = self.get_file_path(key);
        if !file_path.exists() {
            return Ok(false);
        }

        let metadata = tokio_fs::metadata(&file_path).await?;
        let file_size = metadata.len();

        // 检查范围是否完全在文件内
        Ok(range.0 < file_size && range.1 <= file_size)
    }
} 