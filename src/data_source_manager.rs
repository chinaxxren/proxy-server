use std::sync::Arc;
use std::pin::Pin;
use std::path::PathBuf;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use hyper::{Body, Response};
use crate::data_request::DataRequest;
use crate::utils::error::{Result, ProxyError};
use crate::storage::{StorageManager, StorageManagerConfig, DiskStorage, StorageConfig};
use crate::handlers::{CacheHandler, NetworkHandler, MixedSourceHandler, ResponseBuilder};
use crate::log_info;

pub struct DataSourceManager {
    cache_handler: Arc<CacheHandler>,
    network_handler: NetworkHandler,
    mixed_source_handler: MixedSourceHandler,
    response_builder: ResponseBuilder,
}

impl DataSourceManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        log_info!("Cache", "初始化数据源管理器，缓存目录: {:?}", cache_dir);
        
        let storage_config = StorageConfig {
            root_path: cache_dir.clone(),
            chunk_size: 8192,
        };
        
        let manager_config = StorageManagerConfig::default();
        let storage_engine = DiskStorage::new(storage_config);
        let storage_manager = Arc::new(StorageManager::new(storage_engine, manager_config));
        
        let cache_handler = Arc::new(CacheHandler::new(storage_manager));
        let network_handler = NetworkHandler::new();
        let mixed_source_handler = MixedSourceHandler::new(cache_handler.clone());
        let response_builder = ResponseBuilder::new();
        
        Self {
            cache_handler,
            network_handler,
            mixed_source_handler,
            response_builder,
        }
    }
    
    pub async fn process_request(&self, req: &DataRequest) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();
        let key = url.to_string();
        let (start, end) = crate::utils::range::parse_range(&range)?;
        
        log_info!("Cache", "开始处理请求: {} 范围: {}-{}", url, start, end);
        
        // 检查缓存中是否有完整的数据
        if let Ok(has_range) = self.cache_handler.check_range(&key, (start, end)).await {
            if has_range {
                log_info!("Cache", "从缓存读取数据: {} 范围: {}-{}", url, start, end);
                if let Ok(stream) = self.cache_handler.read(&key, (start, end)).await {
                    // 获取文件总大小
                    let range_str = format!("bytes=0-0");
                    let (resp, _, total_size) = self.network_handler.fetch(url, &range_str).await?;
                    let headers = self.network_handler.extract_headers(&resp);
                    
                    return Ok(self.response_builder.build_partial_content_response(
                        stream,
                        headers,
                        start,
                        end,
                        total_size,
                    ));
                }
            }
        }
        
        // 获取缓存文件大小
        let cached_size = self.cache_handler.get_size(&key).await?.unwrap_or(0);
        
        // 如果请求的范围部分在缓存中，部分需要从网络获取
        if cached_size > start {
            // 计算缓存部分的结束位置
            let cached_end = if cached_size >= end {
                end  // 如果缓存数据足够，直接使用请求的结束位置
            } else {
                cached_size  // 使用缓存的最后一个字节位置
            };
            
            // 如果缓存部分有效（至少有一个字节需要从缓存读取）
            if cached_end > start {
                // 检查是否需要从网络获取数据
                if cached_end >= end {
                    // 如果不需要从网络获取，直接返回缓存数据
                    log_info!("Cache", "完全从缓存读取: {}-{}", start, end);
                    if let Ok(stream) = self.cache_handler.read(&key, (start, end)).await {
                        // 获取文件总大小
                        let range_str = format!("bytes=0-0");
                        let (resp, _, total_size) = self.network_handler.fetch(url, &range_str).await?;
                        let headers = self.network_handler.extract_headers(&resp);
                        
                        return Ok(self.response_builder.build_partial_content_response(
                            stream,
                            headers,
                            start,
                            end,
                            total_size,
                        ));
                    }
                }
                
                // 处理混合源请求
                return self.mixed_source_handler.handle(url, &key, start, end, cached_end).await;
            }
        }
        
        // 完全从网络获取
        log_info!("Cache", "开始从网络获取: {} {}-{}", url, start, end);
        let (resp, _, total_size) = self.network_handler.fetch(url, &range).await?;
        let headers = self.network_handler.extract_headers(&resp);
        let (_, body) = resp.into_parts();
        
        // 将 body 转换为我们需要的格式
        let stream = futures::StreamExt::map(Body::wrap_stream(body), |result| {
            result.map_err(|e| ProxyError::Network(e.to_string()))
        });
        let stream = Box::pin(stream);
        
        // 创建两个独立的流
        let (mut tx1, rx1) = futures::channel::mpsc::channel::<Result<Bytes>>(32);
        let (mut tx2, rx2) = futures::channel::mpsc::channel::<Result<Bytes>>(32);
        
        // 启动转发任务
        let forward_handle = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(result) = stream.next().await {
                match result {
                    Ok(chunk) => {
                        if tx1.try_send(Ok(chunk.clone())).is_err() || 
                           tx2.try_send(Ok(chunk)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx1.try_send(Err(e.clone()));
                        let _ = tx2.try_send(Err(e));
                        break;
                    }
                }
            }
        });
        
        // 启动缓存写入
        let cache_stream = Box::pin(futures::StreamExt::map(rx1, |x| x)) as Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;
        let response_stream = Box::new(futures::StreamExt::map(rx2, |x| x)) as Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>;
        
        // 启动缓存写入任务
        let key_clone = key.clone();
        let cache_handler = self.cache_handler.clone();
        let cache_handle = tokio::spawn(async move {
            cache_handler.write_stream(&key_clone, (start, end), cache_stream).await
        });
        
        // 构建响应
        let response = self.response_builder.build_partial_content_response(
            response_stream,
            headers,
            start,
            end,
            total_size,
        );

        // 等待转发任务完成
        if let Err(e) = forward_handle.await {
            log_info!("Cache", "转发任务失败: {}", e);
        }

        // 等待缓存写入任务完成
        if let Err(e) = cache_handle.await {
            log_info!("Cache", "缓存写入任务失败: {}", e);
            // 处理缓存写入失败的情况，但仍然返回响应
            log_info!("Cache", "继续返回响应，尽管缓存写入失败");
        }
        
        Ok(response)
    }
}
