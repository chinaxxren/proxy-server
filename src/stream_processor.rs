use crate::utils::error::Result;
use crate::{log_error, log_info};
use bytes::Bytes;
use futures_util::stream::{IntoStream, TryStreamExt};
use hyper::Body;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use crate::cache::unit_pool::UnitPool;

pub struct StreamProcessor {
    start_pos: u64,
    unit_pool: Arc<UnitPool>,
    url: String,
}

impl StreamProcessor {
    pub fn new(start_pos: u64, url: String, unit_pool: Arc<UnitPool>) -> Self {
        Self {
            start_pos,
            unit_pool,
            url,
        }
    }

    pub async fn process_stream(
        &self,
        body: Body,
        cache_writer: Arc<Mutex<Option<File>>>,
    ) -> Result<Body> {
        log_info!("StreamProcessor", "process_stream");

        let (mut sender, body_ret) = Body::channel();
        let mut current_pos = self.start_pos;
        let url = self.url.clone();
        let unit_pool = self.unit_pool.clone();

        let mut stream = body.into_stream();
        
        tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(64 * 1024);
            
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(data) => {
                        log_info!("StreamProcessor", "data length: {:?}", data.len());
                        
                        // 立即发送数据
                        if let Err(e) = sender.send_data(data.clone()).await {
                            log_error!("StreamProcessor", "发送数据失败: {}", e);
                            break;
                        }
                        log_info!("StreamProcessor", "发送数据成功");
                        
                        // 缓存处理
                        buffer.extend_from_slice(&data);
                        if buffer.len() >= 64 * 1024 {
                            let range = format!("bytes={}-{}", current_pos, current_pos + buffer.len() as u64 - 1);
                            if let Err(e) = Self::write_to_cache(
                                cache_writer.clone(),
                                current_pos,
                                &buffer,
                            ).await {
                                log_error!("StreamProcessor", "写入缓存失败: {}", e);
                            } else {
                                log_info!("StreamProcessor", "写入缓存成功: {} bytes", buffer.len());
                                // 更新缓存状态
                                if let Err(e) = unit_pool.update_cache(&url, &range).await {
                                    log_error!("StreamProcessor", "更新缓存状态失败: {}", e);
                                }
                            }
                            current_pos += buffer.len() as u64;
                            buffer.clear();
                        }
                    }
                    Err(e) => {
                        log_error!("StreamProcessor", "读取数据流失败: {}", e);
                        break;
                    }
                }
            }

            // 处理剩余数据
            if !buffer.is_empty() {
                let range = format!("bytes={}-{}", current_pos, current_pos + buffer.len() as u64 - 1);
                if let Err(e) = Self::write_to_cache(
                    cache_writer,
                    current_pos,
                    &buffer,
                ).await {
                    log_error!("StreamProcessor", "写入最后的缓存失败: {}", e);
                } else {
                    log_info!("StreamProcessor", "写入最后的缓存成功: {} bytes", buffer.len());
                    // 更新缓存状态
                    if let Err(e) = unit_pool.update_cache(&url, &range).await {
                        log_error!("StreamProcessor", "更新最后的缓存状态失败: {}", e);
                    }
                }
            }
        });

        Ok(body_ret)
    }

    pub async fn merge_streams(
        &self,
        cached_stream: impl futures_util::stream::Stream<Item = Result<Bytes>> + Send + 'static,
        network_body: Body,
        cache_writer: Arc<Mutex<Option<File>>>,
    ) -> Result<Body> {
        let (mut sender, body) = Body::channel();
        let mut current_pos = self.start_pos;
        let url = self.url.clone();
        let unit_pool = self.unit_pool.clone();

        tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(64 * 1024);
            
            // 处理缓存流
            let mut cached = Box::pin(cached_stream);
            while let Some(chunk) = cached.next().await {
                match chunk {
                    Ok(data) => {
                        if let Err(e) = sender.send_data(data.clone()).await {
                            log_error!("Stream", "发送缓存数据失败: {}", e);
                            return;
                        }
                        current_pos += data.len() as u64;
                    }
                    Err(e) => {
                        log_error!("Stream", "读取缓存数据失败: {}", e);
                        break;
                    }
                }
            }

            // 处理网络流
            let mut network = network_body.into_stream();
            while let Some(chunk) = network.next().await {
                match chunk {
                    Ok(data) => {
                        // 立即发送数据
                        if let Err(e) = sender.send_data(data.clone()).await {
                            log_error!("Stream", "发送网络数据失败: {}", e);
                            break;
                        }
                        
                        // 缓存处理
                        buffer.extend_from_slice(&data);
                        if buffer.len() >= 64 * 1024 {
                            let range = format!("bytes={}-{}", current_pos, current_pos + buffer.len() as u64 - 1);
                            if let Err(e) = Self::write_to_cache(
                                cache_writer.clone(),
                                current_pos,
                                &buffer,
                            ).await {
                                log_error!("Stream", "写入缓存失败: {}", e);
                            } else {
                                // 更新缓存状态
                                if let Err(e) = unit_pool.update_cache(&url, &range).await {
                                    log_error!("Stream", "更新缓存状态失败: {}", e);
                                }
                            }
                            current_pos += buffer.len() as u64;
                            buffer.clear();
                        }
                    }
                    Err(e) => {
                        log_error!("Stream", "读取网络数据失败: {}", e);
                        break;
                    }
                }
            }

            // 处理剩余数据
            if !buffer.is_empty() {
                let range = format!("bytes={}-{}", current_pos, current_pos + buffer.len() as u64 - 1);
                if let Err(e) = Self::write_to_cache(
                    cache_writer,
                    current_pos,
                    &buffer,
                ).await {
                    log_error!("Stream", "写入最后的缓存失败: {}", e);
                } else {
                    // 更新缓存状态
                    if let Err(e) = unit_pool.update_cache(&url, &range).await {
                        log_error!("Stream", "更新最后的缓存状态失败: {}", e);
                    }
                }
            }
        });

        Ok(body)
    }

    async fn write_to_cache(
        cache_writer: Arc<Mutex<Option<File>>>,
        position: u64,
        data: &[u8],
    ) -> Result<()> {
        log_info!("StreamProcessor", "write_to_cache");
        let mut writer = cache_writer.lock().await;
        if let Some(file) = writer.as_mut() {
            file.seek(std::io::SeekFrom::Start(position)).await?;
            file.write_all(data).await?;
            file.flush().await?;
        }
        log_info!("StreamProcessor", "write_to_cache end");
        Ok(())
    }
}
