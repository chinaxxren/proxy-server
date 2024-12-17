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

const BUFFER_SIZE: usize = 8192;
const MIN_CHUNK_SIZE: usize = 1024;  // 最小数据块大小
const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

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

    async fn collect_data<S>(stream: &mut S, min_size: usize) -> Result<Option<Vec<u8>>>
    where
        S: Stream<Item = Result<Bytes>> + Unpin,
    {
        let mut buffer = Vec::new();
        let mut total_size = 0;
        
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    total_size += chunk.len();
                    buffer.extend_from_slice(&chunk);
                    log_info!("Cache", "接收数据: {} 字节, 累计: {} 字节", chunk.len(), total_size);
                    if buffer.len() >= min_size {
                        return Ok(Some(buffer));
                    }
                }
                Err(e) => return Err(e),
            }
        }
        
        if buffer.is_empty() {
            Ok(None)
        } else {
            Ok(Some(buffer))
        }
    }

    fn prepare_chunk(data: &[u8], chunk_size: usize) -> Vec<u8> {
        if data.len() <= chunk_size {
            data.to_vec()
        } else {
            data[..chunk_size].to_vec()
        }
    }

    pub async fn handle(&self, url: &str, key: &str, start: u64, end: u64, cached_end: u64) -> Result<Response<Body>> {
        log_info!("Cache", "混合源请求 - 缓存: {}-{}, 网络: {}-{}", start, cached_end, cached_end + 1, end);

        // 预先发起网络请求，避免后续延迟
        let network_future = self.network_handler.fetch(url, &format!("bytes={}-{}", cached_end + 1, end));
        let network_result = timeout(NETWORK_TIMEOUT, network_future).await
            .map_err(|_| ProxyError::Network("网络请求超时".to_string()))?;
        let (resp, _, total_size) = network_result?;
        let headers = self.network_handler.extract_headers(&resp);
        let (_, body) = resp.into_parts();
        
        let network_stream = futures::StreamExt::map(Body::wrap_stream(body), |result| {
            result.map_err(|e| ProxyError::Network(e.to_string()))
        });

        // 从缓存读取第一部分
        let cache_stream = self.cache_handler.read(key, (start, cached_end)).await?;

        // 创建合并的流
        let combined_stream = Box::pin(async_stream::stream! {
            let mut cache_stream = cache_stream;
            let mut network_stream = Box::pin(network_stream);
            let mut total_bytes = 0;
            let mut buffer = Vec::with_capacity(BUFFER_SIZE * 2);

            // 处理缓存流
            if let Ok(Some(cache_data)) = Self::collect_data(&mut cache_stream, 0).await {
                log_info!("Cache", "从缓存读取数据: {} 字节", cache_data.len());
                buffer.extend(cache_data);
            }

            // 预读一些网络数据以确保有足够的数据进行处理
            if let Ok(Some(network_data)) = Self::collect_data(&mut network_stream, MIN_CHUNK_SIZE).await {
                log_info!("Cache", "预读网络数据: {} 字节", network_data.len());
                buffer.extend(network_data);
            }

            // 发送第一个数据块
            while buffer.len() >= MIN_CHUNK_SIZE {
                let chunk_size = if buffer.len() >= BUFFER_SIZE {
                    BUFFER_SIZE
                } else {
                    buffer.len()
                };

                let chunk = Self::prepare_chunk(&buffer, chunk_size);
                buffer = buffer[chunk.len()..].to_vec();
                
                log_info!("Cache", "发送数据块: {} 字节", chunk.len());
                total_bytes += chunk.len();
                yield Ok(Bytes::from(chunk));
            }

            // 继续处理网络流
            while let Ok(Some(network_data)) = Self::collect_data(&mut network_stream, BUFFER_SIZE).await {
                buffer.extend(network_data);

                while buffer.len() >= BUFFER_SIZE {
                    let chunk = Self::prepare_chunk(&buffer, BUFFER_SIZE);
                    buffer = buffer[BUFFER_SIZE..].to_vec();
                    
                    log_info!("Cache", "发送数据块: {} 字节", chunk.len());
                    total_bytes += chunk.len();
                    yield Ok(Bytes::from(chunk));
                }
            }

            // 发送剩余数据
            if !buffer.is_empty() {
                log_info!("Cache", "发送最后的数据块: {} 字节", buffer.len());
                total_bytes += buffer.len();
                yield Ok(Bytes::from(buffer));
            }

            log_info!("Cache", "数据传输完成，总计发送: {} 字节", total_bytes);
        });

        Ok(self.response_builder.build_partial_content_response(
            combined_stream,
            headers,
            start,
            end,
            total_size,
        ))
    }
} 