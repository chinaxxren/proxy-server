use std::sync::Arc;
use std::pin::Pin;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use hyper::Body;
use tokio::sync::mpsc;
use crate::storage::{StorageManager, DiskStorage};
use crate::utils::error::{Result, ProxyError};
use crate::log_info;

pub struct CacheHandler {
    storage_manager: Arc<StorageManager<DiskStorage>>,
}

impl CacheHandler {
    pub fn new(storage_manager: Arc<StorageManager<DiskStorage>>) -> Self {
        Self { storage_manager }
    }

    pub async fn check_range(&self, key: &str, range: (u64, u64)) -> Result<bool> {
        self.storage_manager.check_range(key, range).await
    }

    pub async fn get_size(&self, key: &str) -> Result<Option<u64>> {
        self.storage_manager.get_size(key).await
    }

    pub async fn read(&self, key: &str, range: (u64, u64)) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>> {
        self.storage_manager.read(key, range).await
    }

    pub async fn write_stream(
        &self,
        key: &str,
        range: (u64, u64),
        mut stream: Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>,
    ) -> Result<()> {
        let (tx_storage, mut rx_storage) = mpsc::channel::<Bytes>(32);
        let storage_manager = self.storage_manager.clone();
        let key = key.to_string();
        let key_for_process = key.clone();

        // 启动数据处理任务
        let process_handle = tokio::spawn(async move {
            let mut total_bytes = 0u64;
            let mut chunk_count = 0;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        chunk_count += 1;
                        let chunk_size = chunk.len();
                        total_bytes += chunk_size as u64;

                        log_info!("Cache", "接收到数据块 #{}: 大小 {} 字节, 总计 {} 字节", 
                            chunk_count, chunk_size, total_bytes);

                        if tx_storage.send(chunk).await.is_err() {
                            log_info!("Cache", "存储流已关闭: {}", key_for_process);
                            return Err(String::from("存储流已关闭"));
                        }
                    }
                    Err(e) => {
                        log_info!("Cache", "处理数据流错误: {} - {}", key_for_process, e);
                        return Err(e.to_string());
                    }
                }
            }

            log_info!("Cache", "数据流处理完成: {} - 总计处理 {} 个数据块, {} 字节", 
                key_for_process, chunk_count, total_bytes);
            drop(tx_storage);
            Ok(())
        });

        // 启动存储写入任务
        let mut buffer = Vec::new();
        let mut total_written = 0u64;

        while let Some(chunk) = rx_storage.recv().await {
            buffer.extend_from_slice(&chunk);

            if buffer.len() >= 1024 * 64 { // 64KB
                let buffer_size = buffer.len();
                log_info!("Cache", "缓冲区达到写入阈值: {} 字节, 开始写入存储", buffer_size);

                let data = std::mem::take(&mut buffer);
                let stream = Box::pin(futures::stream::once(async move { Ok(Bytes::from(data)) }));
                match storage_manager.write(&key, stream, (range.0 + total_written, range.1)).await {
                    Ok(written) => {
                        total_written += written;
                        log_info!("Cache", "成功写入存储: {} 字节, 总计: {} 字节", written, total_written);
                    }
                    Err(e) => {
                        log_info!("Cache", "写入缓存失败: {} - {}", key, e);
                        return Err(ProxyError::Cache(format!("写入缓存失败: {}", e)));
                    }
                }
            }
        }

        // 写入剩余的数据
        if !buffer.is_empty() {
            let buffer_size = buffer.len();
            log_info!("Cache", "写入剩余数据: {} 字节", buffer_size);

            let stream = Box::pin(futures::stream::once(async move { Ok(Bytes::from(buffer)) }));
            match storage_manager.write(&key, stream, (range.0 + total_written, range.1)).await {
                Ok(written) => {
                    total_written += written;
                    log_info!("Cache", "成功写入最后的数据块: {} 字节, 总计: {} 字节", written, total_written);
                }
                Err(e) => {
                    log_info!("Cache", "写入最后的数据块失败: {} - {}", key, e);
                    return Err(ProxyError::Cache(format!("写入最后的数据块失败: {}", e)));
                }
            }
        }

        // 等待处理任务完成
        match process_handle.await {
            Ok(Ok(())) => {
                log_info!("Cache", "存储写入任务完成: {} - 总计写入: {} 字节", key, total_written);
                Ok(())
            }
            Ok(Err(e)) => {
                log_info!("Cache", "数据处理任务失败: {} - {}", key, e);
                Err(ProxyError::Cache(format!("数据处理任务失败: {}", e)))
            }
            Err(e) => {
                log_info!("Cache", "数据处理任务异常终止: {} - {}", key, e);
                Err(ProxyError::Cache(format!("数据处理任务异常终止: {}", e)))
            }
        }
    }
} 