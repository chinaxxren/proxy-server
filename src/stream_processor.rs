use crate::config::CONFIG;
use crate::utils::error::{ProxyError, Result};
use crate::cache::unit_pool::UnitPool;
use crate::{log_error, log_info};
use bytes::Bytes;
use futures_util::stream::{Stream, StreamExt};
use hyper::Body;
use hyper::body::HttpBody;
use std::io::SeekFrom;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub struct StreamProcessor {
    start_pos: u64,
    end_pos: u64,
    url: String,
    unit_pool: Arc<UnitPool>,
}

impl StreamProcessor {
    pub fn new(start: u64, end: u64, url: String, unit_pool: Arc<UnitPool>) -> Self {
        Self {
            start_pos: start,
            end_pos: end,
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
        let end_pos = self.end_pos;
        let total_size = end_pos - start_pos + 1;

        tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(total_size as usize);
            let mut current_pos = start_pos;
            let mut total_received = 0u64;
            
            'receive_loop: while let Some(chunk) = body.data().await {
                match chunk {
                    Ok(data) => {
                        let available_len = data.len() as u64;
                        let remaining = total_size - total_received;
                        let to_send = std::cmp::min(available_len, remaining) as usize;

                        if to_send == 0 {
                            break 'receive_loop;
                        }

                        let chunk_to_send = if to_send < data.len() {
                            data.slice(0..to_send)
                        } else {
                            data
                        };

                        log_info!("Stream", "接收数据: {} bytes", to_send);
                        
                        // 1. 发送数据给客户端
                        if let Err(e) = sender.send_data(chunk_to_send.clone()).await {
                            log_error!("Stream", "发送数据失败: {}", e);
                            break 'receive_loop;
                        }
                        
                        // 2. 缓存处理
                        buffer.extend_from_slice(&chunk_to_send);
                        total_received += to_send as u64;

                        if total_received >= total_size {
                            log_info!("Stream", "达到请求范围结束位置，停止获取数据");
                            break 'receive_loop;
                        }
                    }
                    Err(e) => {
                        log_error!("Stream", "读取数据流失败: {}", e);
                        break 'receive_loop;
                    }
                }
            }

            // 处理所有数据
            if !buffer.is_empty() {
                log_info!("Stream", "开始处理缓存写入...");
                
                // 先写入缓存
                match Self::write_to_cache(cache_writer, start_pos, &buffer).await {
                    Ok(_) => {
                        log_info!("Stream", "写入数据到缓存: {} bytes at position {}", buffer.len(), start_pos);
                        
                        // 再更新缓存状态
                        let range = format!("bytes={}-{}", start_pos, start_pos + total_received - 1);
                        if let Err(e) = unit_pool.update_cache(&url, &range).await {
                            log_error!("Stream", "更新缓存状态失败: {}", e);
                        } else {
                            log_info!("Stream", "成功更新缓存范围: {}", range);
                        }
                    }
                    Err(e) => {
                        log_error!("Stream", "写入缓存失败: {}", e);
                    }
                }
            }

            log_info!("Stream", "数据处理完成，总共接收: {} bytes", total_received);
            drop(sender);  // 显式关闭发送器
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
        let end_pos = self.end_pos;
        let total_requested = if end_pos == u64::MAX {
            if let Some(state) = unit_pool.cache_map.read().await.get(&CONFIG.get_cache_file(&url)) {
                if let Some(total_size) = state.total_size {
                    total_size - start_pos
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            end_pos - start_pos + 1
        };

        let pos_str = format!("{}", end_pos);
        let temp = if end_pos == u64::MAX { "END" } else { pos_str.as_str() };
        log_info!(
            "Stream",
            "开始处理混合源请求: {} -> {} (请求范围大小: {} bytes)",
            start_pos,
            temp,
            total_requested
        );

        // 1. 处理缓存流
        let mut cached = Box::pin(cached_stream);
        tokio::spawn(async move {
            let mut buffer = Vec::<u8>::new();
            let mut cached_bytes = 0u64;
            let mut network_bytes = 0u64;
            let mut current_pos = start_pos;

            // 获取缓存边界
            let cache_path = CONFIG.get_cache_file(&url);
            let cache_map = unit_pool.cache_map.read().await;
            let cache_end = match cache_map.get(&cache_path) {
                Some(state) => state
                    .ranges
                    .iter()
                    .filter(|range| range.1 >= start_pos && range.0 <= end_pos)
                    .map(|range| range.1)
                    .max()
                    .unwrap_or(0),
                None => 0,
            };
            drop(cache_map);

            // 先读取缓存数据
            log_info!("Stream", "开始读取缓存数据...");
            while let Some(chunk) = cached.next().await {
                match chunk {
                    Ok(data) => {
                        // 算这次应该发送的数据长度
                        let available_len = data.len() as u64;
                        let remaining_needed = if current_pos <= cache_end {
                            std::cmp::min(cache_end + 1 - current_pos, total_requested - cached_bytes)
                        } else {
                            0
                        };
                        let to_send = std::cmp::min(available_len, remaining_needed) as usize;

                        if to_send > 0 {
                            // 只发送需要的部分
                            let chunk_to_send = if to_send < data.len() {
                                data.slice(0..to_send)
                            } else {
                                data
                            };

                            if let Err(e) = sender.send_data(chunk_to_send).await {
                                log_error!(
                                    "Stream",
                                    "发送缓存数据失败: {} (已发送: {} bytes)",
                                    e,
                                    cached_bytes
                                );
                                return;
                            }
                            cached_bytes += to_send as u64;
                            current_pos += to_send as u64;

                            log_info!(
                                "Stream",
                                "发送缓存数据: {} bytes, 累计: {} bytes ({}%)",
                                to_send,
                                cached_bytes,
                                (cached_bytes as f64 / total_requested as f64 * 100.0) as u64
                            );
                        }

                        if current_pos > cache_end || cached_bytes >= total_requested {
                            break;
                        }
                    }
                    Err(e) => {
                        log_error!(
                            "Stream",
                            "读取缓存数据失败: {} (已读取: {} bytes)",
                            e,
                            cached_bytes
                        );
                        break;
                    }
                }
            }

            // 2. 处理网络流
            if cached_bytes < total_requested {
                let network_start = start_pos + cached_bytes;
                let remaining_bytes = total_requested - cached_bytes;

                log_info!(
                    "Stream",
                    "需要从网络获取数据: {} -> {} ({}bytes)",
                    network_start,
                    network_start + remaining_bytes - 1,
                    remaining_bytes
                );

                let mut buffer = Vec::with_capacity(remaining_bytes as usize);
                let mut bytes_received = 0u64;

                while let Some(chunk) = network_stream.data().await {
                    match chunk {
                        Ok(data) => {
                            let available_len = data.len() as u64;
                            let to_send = std::cmp::min(available_len, remaining_bytes - bytes_received) as usize;

                            if to_send > 0 {
                                let chunk_to_send = if to_send < data.len() {
                                    data.slice(0..to_send)
                                } else {
                                    data
                                };

                                // 发送数据给客户端
                                if let Err(e) = sender.send_data(chunk_to_send.clone()).await {
                                    log_error!(
                                        "Stream",
                                        "发送网络数据失败: {} (已发送: {} bytes)",
                                        e,
                                        bytes_received
                                    );
                                    break;
                                }

                                // 更新计数器和缓存
                                buffer.extend_from_slice(&chunk_to_send);
                                bytes_received += to_send as u64;
                                network_bytes += to_send as u64;

                                log_info!(
                                    "Stream",
                                    "发送网络数据: {} bytes, 累计: {} bytes ({}%)",
                                    to_send,
                                    bytes_received,
                                    (bytes_received as f64 / remaining_bytes as f64 * 100.0) as u64
                                );

                                if bytes_received >= remaining_bytes {
                                    log_info!(
                                        "Stream",
                                        "达到请求范围结束位置，停止获取数据 (总计获取: {} bytes)",
                                        bytes_received
                                    );
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            log_error!(
                                "Stream",
                                "读取网络数据失败: {} (已获取: {} bytes)",
                                e,
                                bytes_received
                            );
                            break;
                        }
                    }
                }

                // 写入缓存
                if !buffer.is_empty() {
                    log_info!(
                        "Stream",
                        "准备写入网络数据到缓存: {} bytes at position {}",
                        buffer.len(),
                        network_start
                    );

                    if let Err(e) = Self::write_to_cache(cache_writer, network_start, &buffer).await {
                        log_error!("Stream", "写入缓存失败: {} (数据大小: {} bytes)", e, buffer.len());
                    } else {
                        log_info!("Stream", "成功写入网络数据到缓存: {} bytes at position {}", buffer.len(), network_start);
                        
                        // 更新缓存状态
                        let range = format!("bytes={}-{}", network_start, network_start + bytes_received - 1);
                        if let Err(e) = unit_pool.update_cache(&url, &range).await {
                            log_error!("Stream", "更新缓存状态失败: {} (范围: {})", e, range);
                        } else {
                            log_info!("Stream", "成功更新最终缓存范围: {} -> {} (总计: {} bytes)", 
                                network_start, network_start + bytes_received - 1, bytes_received);
                        }
                    }
                }
            }

            log_info!(
                "Stream",
                "数据处理完成 - 缓存: {} bytes, 网络: {} bytes, 总计: {} bytes",
                cached_bytes,
                network_bytes,
                cached_bytes + network_bytes
            );
        });

        Ok(body_ret)
    }

    // 写入缓存文件
    async fn write_to_cache(
        cache_writer: Arc<Mutex<Option<File>>>,
        start_pos: u64,
        data: &[u8],
    ) -> Result<()> {
        let mut file_guard = cache_writer.lock().await;
        if let Some(file) = file_guard.as_mut() {
            // 移动到正确的写入位置
            file.seek(SeekFrom::Start(start_pos)).await?;
            
            // 写入数据
            file.write_all(data).await?;
            file.flush().await?;
            
            log_info!("Cache", "写入缓存: {} bytes at position {}", data.len(), start_pos);
            Ok(())
        } else {
            log_error!("Cache", "缓存文件不可用");
            Err(ProxyError::Cache("缓存文件不可用".to_string()))
        }
    }
}
