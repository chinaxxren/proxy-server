use crate::utils::error::Result;
use crate::log_error;
use futures::Stream;
use hyper::Body;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, AsyncSeekExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use bytes::Bytes;
use futures::StreamExt;

pub struct StreamProcessor {
    buffer_size: usize,
    cache_path: String,
    start_pos: u64,
    total_size: u64,
}

impl StreamProcessor {
    pub fn new(cache_path: String, start_pos: u64, total_size: u64) -> Self {
        Self {
            buffer_size: 64 * 1024,
            cache_path,
            start_pos,
            total_size,
        }
    }

    pub async fn process_stream(
        &self,
        body: Body,
        cache_writer: Arc<Mutex<Option<File>>>,
    ) -> Result<Body> {
        let (mut sender, body_ret) = Body::channel();
        let current_pos = self.start_pos;

        let bytes = hyper::body::to_bytes(body).await?;
        
        if let Err(e) = Self::write_to_cache(
            cache_writer.clone(),
            current_pos,
            &bytes,
        ).await {
            log_error!("Cache", "写入缓存失败: {}", e);
        }
        
        if let Err(e) = sender.send_data(bytes).await {
            log_error!("Stream", "发送数据失败: {}", e);
        }

        Ok(body_ret)
    }

    pub async fn merge_streams(
        &self,
        cached_stream: impl Stream<Item = Result<Bytes>> + Send + 'static,
        network_body: Body,
        cache_writer: Arc<Mutex<Option<File>>>,
    ) -> Result<Body> {
        let (mut sender, body) = Body::channel();
        let mut cached = Box::pin(cached_stream);
        let mut current_pos = self.start_pos;
        let mut buffer = Vec::with_capacity(64 * 1024);

        // 先读取整个网络 body
        let network_bytes = hyper::body::to_bytes(network_body).await?;

        tokio::spawn(async move {
            // 处理缓存流
            while let Some(chunk) = cached.next().await {
                match chunk {
                    Ok(data) => {
                        current_pos += data.len() as u64;
                        if let Err(e) = sender.send_data(data).await {
                            log_error!("Stream", "发送缓存数据失败: {}", e);
                            return;
                        }
                    }
                    Err(e) => {
                        log_error!("Stream", "读取缓存数据失败: {}", e);
                        break;
                    }
                }
            }

            // 处理网络数据
            buffer.extend_from_slice(&network_bytes);
            if let Err(e) = Self::write_to_cache(
                cache_writer.clone(),
                current_pos,
                &buffer,
            ).await {
                log_error!("Cache", "写入缓存失败: {}", e);
            }
            
            if let Err(e) = sender.send_data(Bytes::from(buffer)).await {
                log_error!("Stream", "发送网络数据失败: {}", e);
            }
        });

        Ok(body)
    }

    async fn write_to_cache(
        cache_writer: Arc<Mutex<Option<File>>>,
        position: u64,
        data: &[u8],
    ) -> Result<()> {
        let mut writer = cache_writer.lock().await;
        if let Some(file) = writer.as_mut() {
            file.seek(std::io::SeekFrom::Start(position)).await?;
            file.write_all(data).await?;
            file.flush().await?;
        }
        Ok(())
    }
} 