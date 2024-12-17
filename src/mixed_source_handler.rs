use std::sync::Arc;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use hyper::{Body, Response};
use tokio::sync::Mutex;
use crate::cache_handler::CacheHandler;
use crate::network_handler::NetworkHandler;
use crate::response_builder::ResponseBuilder;
use crate::utils::error::{Result, ProxyError};
use crate::log_info;

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
        log_info!("Cache", "混合源请求 - 缓存: {}-{}, 网络: {}-{}", 
            start, cached_end, cached_end + 1, end);

        // 从缓存读取已有部分
        let cached_stream = self.cache_handler.read(key, (start, cached_end)).await?;

        // 从网络获取剩余部分
        let range_str = format!("bytes={}-{}", cached_end + 1, end);
        let (resp, _, total_size) = self.network_handler.fetch(url, &range_str).await?;
        let headers = self.network_handler.extract_headers(&resp);
        let (_, body) = resp.into_parts();

        // 创建混合响应流
        let response_stream = self.create_mixed_stream(cached_stream, body).await;

        // 构建响应
        Ok(self.response_builder.build_partial_content_response(
            Box::new(response_stream),
            headers,
            start,
            end,
            total_size,
        ))
    }

    async fn create_mixed_stream(
        &self,
        cached_stream: Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>,
        network_stream: Body,
    ) -> impl Stream<Item = Result<Bytes>> + Send {
        struct StreamState {
            cached_stream: Option<Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>>,
            network_stream: Option<Body>,
            using_cache: bool,
        }

        let state = StreamState {
            cached_stream: Some(cached_stream),
            network_stream: Some(network_stream),
            using_cache: true,
        };

        futures::stream::unfold(state, move |mut state| async move {
            if state.using_cache {
                if let Some(ref mut stream) = state.cached_stream {
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            log_info!("Cache", "混合流发送缓存数据: {} 字节", chunk.len());
                            return Some((Ok(chunk), state));
                        }
                        Some(Err(e)) => {
                            log_info!("Cache", "缓存数据读取错误: {}", e);
                            state.using_cache = false;
                        }
                        None => {
                            log_info!("Cache", "缓存数据发送完毕，切换到网络数据");
                            state.using_cache = false;
                            state.cached_stream = None;
                        }
                    }
                } else {
                    state.using_cache = false;
                }
            }

            if !state.using_cache {
                if let Some(ref mut stream) = state.network_stream {
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            log_info!("Cache", "混合流发送网络数据: {} 字节", chunk.len());
                            return Some((Ok(chunk), state));
                        }
                        Some(Err(e)) => {
                            log_info!("Cache", "网络数据读取错误: {}", e);
                            state.network_stream = None;
                            return Some((Err(ProxyError::Cache(e.to_string())), state));
                        }
                        None => {
                            log_info!("Cache", "网络数据发送完毕");
                            state.network_stream = None;
                            return None;
                        }
                    }
                }
            }
            None
        })
    }
} 