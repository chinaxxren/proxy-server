use crate::cache::unit_pool::UnitPool;
use crate::cache::CacheManager;
use crate::data_request::DataRequest;
use crate::data_storage::DataStorage;
use crate::stream_processor::StreamProcessor;
use crate::utils::error::{ProxyError, Result};
use crate::{log_error, log_info};
use hyper::header::{HeaderMap, HeaderValue};
use hyper::{Body, Request, Response};
use std::sync::Arc;
use tokio::fs::File;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use bytes::Bytes;
use futures::StreamExt;

pub struct DataSourceManager {
    unit_pool: UnitPool,
    storage: DataStorage,
    cache_manager: CacheManager,
}

impl DataSourceManager {
    pub fn new() -> Self {
        let unit_pool = UnitPool::new();
        let storage = DataStorage::new();
        let cache_manager = CacheManager::new(Arc::new(unit_pool.clone()));

        // 启动定期清理缓存的任务
        let cache_manager_clone = cache_manager.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600 * 24)); // 每24小时清理一次
            loop {
                interval.tick().await;
                if let Err(e) = cache_manager_clone.clean_cache().await {
                    log_error!("Cache", "清理缓存失败: {}", e);
                } else {
                    log_info!("Cache", "缓存清理完成");
                }
            }
        });

        Self {
            unit_pool,
            storage,
            cache_manager,
        }
    }

    pub async fn process_request(&self, req: Request<Body>) -> Result<Response<Body>> {
        // 创建请求对象
        let data_request = DataRequest::new(&req)?;

        // 使用带缓存的请求处理
        self.process_request_with_cache(&data_request).await
    }

    async fn select_data_source(&self, url: &str, range: &str) -> Result<DataSourceType> {
        // 检查网络状态
        if !self.check_network_available().await? {
            return Ok(DataSourceType::FileOnly);
        }

        // 检查缓存状态
        if self.unit_pool.is_fully_cached(url, range).await? {
            log_info!("Cache", "完全缓存");
            Ok(DataSourceType::FileOnly)
        } else if self.unit_pool.is_partially_cached(url, range).await? {
            log_info!("Cache", "部分缓存");
            Ok(DataSourceType::Mixed)
        } else {
            log_info!("Cache", "网络数据");
            Ok(DataSourceType::NetworkOnly)
        }
    }

    async fn check_network_available(&self) -> Result<bool> {
        // 实现简单的网络可用性检查
        let client = hyper::Client::new();
        let uri = hyper::Uri::from_static("http://www.baidu.com");

        match client.get(uri).await {
            Ok(resp) => {
                if resp.status().is_success() {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(_) => {
                log_error!("Network", "网络连接检查失败");
                Ok(false)
            }
        }
    }

    async fn process_request_with_cache(&self, req: &DataRequest) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();

        log_info!("Request", "url: {}", url);
        log_info!("Request", "range: {}", range);

        match self.select_data_source(url, range).await? {
            // 文件缓存
            DataSourceType::FileOnly => {
                let file_source = self.storage.get_file_source(url, range).await?;
                let stream = file_source.read_stream().await?;
                
                // 创建 channel
                let (mut sender, body) = Body::channel();
                
                // 启动异步任务处理流
                tokio::spawn(async move {
                    let mut stream = Box::pin(stream);
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(data) => {
                                if let Err(e) = sender.send_data(data).await {
                                    log_error!("Stream", "发送数据失败: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                log_error!("Stream", "读取数据失败: {}", e);
                                break;
                            }
                        }
                    }
                });

                let headers = self.build_response_headers(range, false);
                let mut response = Response::builder().status(206);

                for (key, value) in headers.iter() {
                    response = response.header(key.as_str(), value);
                }

                response = response.header("Content-Range", format!("bytes {}", range));
                let response = response
                    .body(body)
                    .map_err(|e| ProxyError::Response(e.to_string()))?;
                Ok(response)
            }

            // 网络缓存
            DataSourceType::NetworkOnly => {
                let mut net_source = self.storage.get_net_source(url, range).await?;
                match net_source.download_stream().await {
                    Ok((mut response, total_size)) => {
                        if range.ends_with('-') && total_size > 0 {
                            let start = range[6..range.len() - 1].parse::<u64>().unwrap_or(0);
                            
                            // 创建缓存文件
                            let cache_path = self.storage.get_cache_path(url);
                            let file = File::create(&cache_path).await?;
                            let cache_writer = Arc::new(Mutex::new(Some(file)));
                            
                            // 创建流处理器
                            let processor = StreamProcessor::new(
                                cache_path.to_string_lossy().to_string(),
                                start,
                                total_size,
                            );
                            
                            // 处理响应体
                            let stream = processor.process_stream(response.into_body(), cache_writer).await?;
                            
                            // 构建新的��应
                            let mut headers = self.build_response_headers(range, false);
                            headers.insert(
                                "Content-Range",
                                HeaderValue::from_str(&format!(
                                    "bytes {}-{}/{}",
                                    start,
                                    total_size - 1,
                                    total_size
                                ))
                                .unwrap(),
                            );
                            
                            let response = Response::builder()
                                .status(206)
                                .body(stream)
                                .map_err(|e| ProxyError::Response(e.to_string()))?;
                            Ok(response)
                        } else {
                            Ok(response)
                        }
                    }
                    Err(e) => Err(ProxyError::Network(format!("下载失败: {}", e))),
                }
            }

            // 混合缓存
            DataSourceType::Mixed => {
                let mut net_source = self.storage.get_net_source(url, range).await?;
                let file_source = self.storage.get_file_source(url, range).await?;
                
                match net_source.download_stream().await {
                    Ok((response, total_size)) => {
                        if range.ends_with('-') && total_size > 0 {
                            let start = range[6..range.len() - 1].parse::<u64>().unwrap_or(0);
                            
                            // 创建缓存文件
                            let cache_path = self.storage.get_cache_path(url);
                            let file = File::create(&cache_path).await?;
                            let cache_writer = Arc::new(Mutex::new(Some(file)));
                            
                            // 创建流处理器
                            let processor = StreamProcessor::new(
                                cache_path.to_string_lossy().to_string(),
                                start,
                                total_size,
                            );
                            
                            // 获取缓存流和网络流
                            let cached_stream = file_source.read_stream().await?;
                            let network_stream = response.into_body();
                            
                            // 合并流
                            let merged_stream = processor
                                .merge_streams(cached_stream, network_stream, cache_writer)
                                .await?;
                            
                            // 构建响应
                            let mut headers = self.build_response_headers(range, true);
                            headers.insert(
                                "Content-Range",
                                HeaderValue::from_str(&format!(
                                    "bytes {}-{}/{}",
                                    start,
                                    total_size - 1,
                                    total_size
                                ))
                                .unwrap(),
                            );
                            
                            let response = Response::builder()
                                .status(206)
                                .body(merged_stream)
                                .map_err(|e| ProxyError::Response(e.to_string()))?;
                            Ok(response)
                        } else {
                            Ok(response)
                        }
                    }
                    Err(e) => {
                        // 网络错误时使用缓存数据
                        let stream = file_source.read_stream().await?;
                        
                        // 创建 channel
                        let (mut sender, body) = Body::channel();
                        
                        // 启动异步任务处理流
                        tokio::spawn(async move {
                            let mut stream = Box::pin(stream);
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(data) => {
                                        if let Err(e) = sender.send_data(data).await {
                                            log_error!("Stream", "发送数据失败: {}", e);
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        log_error!("Stream", "读取数据失败: {}", e);
                                        break;
                                    }
                                }
                            }
                        });

                        let mut response = Response::builder()
                            .status(206)
                            .body(body)
                            .map_err(|e| ProxyError::Response(e.to_string()))?;
                        
                        response.headers_mut().insert(
                            "X-Cache-Status",
                            HeaderValue::from_static("partial"),
                        );
                        
                        Ok(response)
                    }
                }
            }
        }
    }

    fn build_response_headers(&self, range: &str, is_partial: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/octet-stream"),
        );

        if !range.is_empty() {
            headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
            if is_partial {
                headers.insert("X-Cache-Status", HeaderValue::from_static("partial"));
            }
        }

        headers
    }
}

impl Default for DataSourceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum DataSourceType {
    FileOnly,
    NetworkOnly,
    Mixed,
}
