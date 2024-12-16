use crate::utils::error::{ProxyError, Result};
use crate::cache::unit_pool::UnitPool;
use crate::{log_error, log_info};
use bytes::Bytes;
use futures_util::stream::{Stream, StreamExt};
use hyper::Body;
use hyper::body::HttpBody;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub struct StreamProcessor {
    start_pos: u64,
    url: String,
    unit_pool: Arc<UnitPool>,
}

impl StreamProcessor {
    pub fn new(start_pos: u64, url: String, unit_pool: Arc<UnitPool>) -> Self {
        Self {
            start_pos,
            url,
            unit_pool,
        }
    }

    // 处理网络流，同时写入缓存
    pub async fn process_stream(&self, mut body: Body, cache_writer: Arc<Mutex<Option<File>>>) -> Result<Body> {
        let (mut sender, body_ret) = Body::channel();
        let url = self.url.clone();
        let unit_pool = self.unit_pool.clone();
        let start_pos = self.start_pos;

        tokio::spawn(async move {
            let mut buffer = Vec::new();
            let mut current_pos = start_pos;
            
            while let Some(chunk) = body.data().await {
                match chunk {
                    Ok(data) => {
                        log_info!("Stream", "接收数据: {} bytes", data.len());
                        
                        // 1. 发送数据给客户端
                        if let Err(e) = sender.send_data(data.clone()).await {
                            log_error!("Stream", "发送数据失败: {}", e);
                            break;
                        }
                        
                        // 2. 缓存处理
                        buffer.extend_from_slice(&data);
                        current_pos += data.len() as u64;
                    }
                    Err(e) => {
                        log_error!("Stream", "读取数据流失败: {}", e);
                        break;
                    }
                }
            }

            // 处理所有数据
            if !buffer.is_empty() {
                let range = format!("bytes={}-{}", start_pos, current_pos - 1);
                if let Err(e) = Self::write_to_cache(
                    cache_writer,
                    start_pos,
                    &buffer,
                ).await {
                    log_error!("Stream", "写入缓存失败: {}", e);
                } else {
                    log_info!("Stream", "写入数据到缓存: {} bytes at position {}", buffer.len(), start_pos);
                    if let Err(e) = unit_pool.update_cache(&url, &range).await {
                        log_error!("Stream", "更新缓存状态失败: {}", e);
                    }
                }
            }
        });

        Ok(body_ret)
    }

    // 合并缓存流和网络流
    pub async fn merge_streams(
        &self,
        cached_stream: impl Stream<Item = Result<Bytes>> + Send + 'static,
        mut network_stream: Body,
        cache_writer: Arc<Mutex<Option<File>>>,
    ) -> Result<Body> {
        let (mut sender, body_ret) = Body::channel();
        let url = self.url.clone();
        let unit_pool = self.unit_pool.clone();
        let start_pos = self.start_pos;
        let end_pos = start_pos + 102400; // 请求范围的结束位置

        log_info!("Stream", "开始处理混合源请求: {} -> {}", start_pos, end_pos);

        // 1. 处理缓存流
        let mut cached = Box::pin(cached_stream);
        tokio::spawn(async move {
            let mut buffer = Vec::new();
            let mut cached_bytes = 0u64;

            // 先读取缓存数据
            while let Some(chunk) = cached.next().await {
                match chunk {
                    Ok(data) => {
                        if let Err(e) = sender.send_data(data.clone()).await {
                            log_error!("Stream", "发送缓存数据失败: {}", e);
                            return;
                        }
                        cached_bytes += data.len() as u64;
                        log_info!("Stream", "发送缓存数据: {} bytes, 总计: {}", data.len(), cached_bytes);
                    }
                    Err(e) => {
                        log_error!("Stream", "读取缓存数据失败: {}", e);
                        break;
                    }
                }
            }

            // 2. 处理网络流
            let network_start = start_pos + cached_bytes; // 从缓存结束的位置开始
            let mut network_pos = network_start;
            
            log_info!("Stream", "缓存数据处理完成，开始处理网络数据: {} -> {}", network_start, end_pos);

            // 检查是否需要从网络获取数据
            if network_pos < end_pos {
                while let Some(chunk) = network_stream.data().await {
                    match chunk {
                        Ok(data) => {
                            // 发送数据给客户端
                            if let Err(e) = sender.send_data(data.clone()).await {
                                log_error!("Stream", "发送网络数据失败: {}", e);
                                break;
                            }
                            log_info!("Stream", "发送网络数据: {} bytes, 位置: {}", data.len(), network_pos);

                            // 缓存处理
                            buffer.extend_from_slice(&data);
                            network_pos += data.len() as u64;

                            // 如果已经达到请求范围的结束位置，停止获取数据
                            if network_pos >= end_pos {
                                log_info!("Stream", "达到请求范围结束位置，停止获取数据");
                                break;
                            }
                        }
                        Err(e) => {
                            log_error!("Stream", "读取网络数据失败: {}", e);
                            break;
                        }
                    }
                }

                // 处理所有数据
                if !buffer.is_empty() {
                    let range = format!("bytes={}-{}", network_start, network_pos - 1);
                    log_info!("Stream", "准备写入网络数据到缓存: {} -> {}", network_start, network_pos - 1);
                    
                    if let Err(e) = Self::write_to_cache(
                        cache_writer,
                        network_start,
                        &buffer,
                    ).await {
                        log_error!("Stream", "写入缓存失败: {}", e);
                    } else {
                        log_info!("Stream", "写入网络数据到缓存: {} bytes at position {}", buffer.len(), network_start);
                        if let Err(e) = unit_pool.update_cache(&url, &range).await {
                            log_error!("Stream", "更新缓存状态失败: {}", e);
                        }
                    }
                }
            } else {
                log_info!("Stream", "无需从网络获取数据，缓存已完整");
            }
        });

        Ok(body_ret)
    }

    // 写入缓存文件
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
            log_info!("Cache", "写入缓存: {} bytes at position {}", data.len(), position);
        }
        Ok(())
    }
}
