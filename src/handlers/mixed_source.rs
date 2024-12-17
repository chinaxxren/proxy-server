use std::pin::Pin;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use hyper::{Body, Response};
use tokio::time::timeout;
use std::time::Duration;
use crate::utils::error::{Result, ProxyError};
use crate::handlers::{CacheHandler, NetworkHandler, ResponseBuilder};
use std::sync::Arc;
use crate::log_info;

const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);
const MIN_CACHE_SIZE: usize = 8192; // 最小缓存处理大小

pub struct MixedSourceHandler {
    cache_handler: Arc<CacheHandler>,
    network_handler: NetworkHandler,
    response_builder: ResponseBuilder,
}

impl MixedSourceHandler {
    pub fn new(cache_handler: Arc<CacheHandler>) -> Self {
        Self {
            cache_handler,
            network_handler: NetworkHandler::new(),
            response_builder: ResponseBuilder::new(),
        }
    }

    pub async fn handle(&self, url: &str, key: &str, start: u64, end: u64, cached_end: u64) -> Result<Response<Body>> {
        log_info!("Cache", "混合源请求开始 - 缓存范围: {}-{}, 网络范围: {}-{}", start, cached_end - 1, cached_end, end);

        // 验证请求范围
        if start > end || cached_end < start || cached_end > end {
            log_info!("Cache", "请求范围无效: start={}, end={}, cached_end={}", start, end, cached_end);
            return Err(ProxyError::InvalidRange("无效的请求范围".to_string()));
        }

        // 计算数据大小
        let cache_size = (cached_end - start) as usize;
        
        // 如果缓存部分太小，直接从网络获取整个范围
        if cache_size < MIN_CACHE_SIZE {
            log_info!("Cache", "缓存范围过小 ({} 字节), 直接从网络获取整个范围: {}-{}", 
                cache_size, start, end);
            
            let range = format!("bytes={}-{}", start, end);
            let network_future = self.network_handler.fetch(url, &range);
            let network_result = timeout(NETWORK_TIMEOUT, network_future).await
                .map_err(|_| {
                    log_info!("Cache", "网络请求超时: {} ({}秒)", url, NETWORK_TIMEOUT.as_secs());
                    ProxyError::Network("网络请求超时".to_string())
                })?;
                
            let (resp, content_length, total_file_size) = match network_result {
                Ok(result) => result,
                Err(e) => {
                    log_info!("Cache", "网络请求失败: {} - {}", url, e);
                    return Err(ProxyError::Network(format!("网络请求失败: {}", e)));
                }
            };

            let headers = self.network_handler.extract_headers(&resp);
            let (_, body) = resp.into_parts();
            
            let network_stream = futures::StreamExt::map(Body::wrap_stream(body), |result| {
                result.map_err(|e| {
                    log_info!("Cache", "网络数据流错误: {}", e);
                    ProxyError::Network(e.to_string())
                })
            });

            log_info!("Cache", "创建响应 - 范围: {}-{}, 总大小: {}", start, end, total_file_size);
            return Ok(self.response_builder.build_partial_content_response(
                Box::new(network_stream),
                headers,
                start,
                end,
                total_file_size,
            ));
        }

        let network_size = (end - cached_end + 1) as usize;
        let total_size = cache_size + network_size;

        log_info!("Cache", "数据大小计算 - 缓存: {} 字节, 网络: {} 字节, 总计: {} 字节", 
            cache_size, network_size, total_size);

        // 预先发起网络请求
        let range = format!("bytes={}-{}", cached_end, end);
        log_info!("Cache", "发起网络请求 - URL: {}, Range: {}", url, range);
        
        let network_future = self.network_handler.fetch(url, &range);
        let network_result = timeout(NETWORK_TIMEOUT, network_future).await
            .map_err(|_| {
                log_info!("Cache", "网络请求超时: {} ({}秒)", url, NETWORK_TIMEOUT.as_secs());
                ProxyError::Network("网络请求超时".to_string())
            })?;
            
        let (resp, content_length, total_file_size) = match network_result {
            Ok(result) => result,
            Err(e) => {
                log_info!("Cache", "网络请求失败: {} - {}", url, e);
                return Err(ProxyError::Network(format!("网络请求失败: {}", e)));
            }
        };

        // 验证网络响应大小
        if content_length != network_size as u64 {
            log_info!("Cache", "警告：网络响应大小不匹配 - 期望: {} 字节, 实际: {} 字节", 
                network_size, content_length);
        }

        let headers = self.network_handler.extract_headers(&resp);
        let (_, body) = resp.into_parts();
        
        let network_stream = futures::StreamExt::map(Body::wrap_stream(body), |result| {
            result.map_err(|e| {
                log_info!("Cache", "网络数据流错误: {}", e);
                ProxyError::Network(e.to_string())
            })
        });

        // 从缓存读取数据
        log_info!("Cache", "开始读取缓存数据 - 文件: {}, 范围: {}-{}", key, start, cached_end - 1);
        let cache_stream = match self.cache_handler.read(key, (start, cached_end - 1)).await {
            Ok(stream) => stream,
            Err(e) => {
                log_info!("Cache", "读取缓存失败: {} - {}", key, e);
                return Err(e);
            }
        };

        // 创建合并的流
        let combined_stream = self.create_mixed_stream(
            cache_stream,
            Box::pin(network_stream),
            cache_size,
            network_size,
        );

        log_info!("Cache", "创建响应 - 范围: {}-{}, 总大小: {}", start, end, total_file_size);
        Ok(self.response_builder.build_partial_content_response(
            Box::new(combined_stream),
            headers,
            start,
            end,
            total_file_size,
        ))
    }

    fn create_mixed_stream(
        &self,
        cached_stream: Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>,
        network_stream: Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>,
        cache_size: usize,
        network_size: usize,
    ) -> impl Stream<Item = Result<Bytes>> + Send + Unpin {
        struct StreamState {
            cached_stream: Option<Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>>,
            network_stream: Option<Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>>,
            using_cache: bool,
            cache_received: usize,
            network_received: usize,
            cache_size: usize,
            network_size: usize,
            error_occurred: bool,
            chunk_count: usize,
        }

        let state = StreamState {
            cached_stream: Some(cached_stream),
            network_stream: Some(network_stream),
            using_cache: true,
            cache_received: 0,
            network_received: 0,
            cache_size,
            network_size,
            error_occurred: false,
            chunk_count: 0,
        };

        Box::pin(futures::stream::unfold(state, move |mut state| async move {
            if state.error_occurred {
                return None;
            }

            if state.using_cache && state.cache_received < state.cache_size {
                if let Some(ref mut stream) = state.cached_stream {
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            let remaining = state.cache_size - state.cache_received;
                            let chunk_size = chunk.len().min(remaining);
                            
                            if chunk_size > 0 {
                                let data = chunk[..chunk_size].to_vec();
                                state.cache_received += chunk_size;
                                state.chunk_count += 1;
                                
                                log_info!("Cache", "发送缓存数据 #{} - 大小: {} 字节, 已发送: {}/{} 字节 ({:.1}%)", 
                                    state.chunk_count,
                                    chunk_size, 
                                    state.cache_received, 
                                    state.cache_size,
                                    (state.cache_received as f64 / state.cache_size as f64 * 100.0));

                                if state.cache_received >= state.cache_size {
                                    state.using_cache = false;
                                    state.cached_stream = None;
                                    state.chunk_count = 0;
                                    log_info!("Cache", "缓存数据发送完毕，切换到网络数据");
                                }

                                return Some((Ok(Bytes::from(data)), state));
                            }
                        }
                        Some(Err(e)) => {
                            log_info!("Cache", "读取缓存数据错误: {}", e);
                            state.error_occurred = true;
                            state.using_cache = false;
                            state.cached_stream = None;
                            return Some((Err(e), state));
                        }
                        None => {
                            if state.cache_received < state.cache_size {
                                log_info!("Cache", "警告：缓存数据不足 - 已接收: {} 字节, 期望: {} 字节", 
                                    state.cache_received, state.cache_size);
                                state.error_occurred = true;
                                return Some((Err(ProxyError::Network("缓存数据不足".to_string())), state));
                            }

                            state.using_cache = false;
                            state.cached_stream = None;
                            state.chunk_count = 0;
                            log_info!("Cache", "缓存数据发送完毕，切换到网络数据");
                        }
                    }
                }
            }

            if !state.using_cache && state.network_received < state.network_size {
                if let Some(ref mut stream) = state.network_stream {
                    match stream.as_mut().next().await {
                        Some(Ok(chunk)) => {
                            let remaining = state.network_size - state.network_received;
                            let chunk_size = chunk.len().min(remaining);
                            
                            if chunk_size > 0 {
                                let data = chunk[..chunk_size].to_vec();
                                state.network_received += chunk_size;
                                state.chunk_count += 1;
                                
                                log_info!("Cache", "发送网络数据 #{} - 大小: {} 字节, 已发送: {}/{} 字节 ({:.1}%)", 
                                    state.chunk_count,
                                    chunk_size, 
                                    state.network_received, 
                                    state.network_size,
                                    (state.network_received as f64 / state.network_size as f64 * 100.0));

                                if state.network_received >= state.network_size {
                                    state.network_stream = None;
                                    log_info!("Cache", "网络数据发送完毕 - 总计发送: {} 字节", state.network_received);
                                }

                                return Some((Ok(Bytes::from(data)), state));
                            }
                        }
                        Some(Err(e)) => {
                            log_info!("Cache", "读取网络数据错误: {}", e);
                            state.error_occurred = true;
                            state.network_stream = None;
                            return Some((Err(e), state));
                        }
                        None => {
                            if state.network_received < state.network_size {
                                log_info!("Cache", "警告：网络数据不足 - 已接收: {} 字节, 期望: {} 字节", 
                                    state.network_received, state.network_size);
                                state.error_occurred = true;
                                return Some((Err(ProxyError::Network("网络数据不足".to_string())), state));
                            }

                            state.network_stream = None;
                            log_info!("Cache", "网络数据发送完毕 - 总计发送: {} 字节", state.network_received);
                            return None;
                        }
                    }
                }
            }

            if state.cache_received >= state.cache_size && state.network_received >= state.network_size {
                log_info!("Cache", "数据传输完成 - 缓存: {} 字节, 网络: {} 字节, 总计: {} 字节",
                    state.cache_received, state.network_received, state.cache_received + state.network_received);
                return None;
            }

            if state.using_cache {
                state.using_cache = false;
                state.cached_stream = None;
                state.chunk_count = 0;
                log_info!("Cache", "缓存数据发送完毕，切换到网络数据");
            }

            None
        }))
    }
} 