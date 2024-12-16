use crate::cache::unit_pool::UnitPool;
use crate::cache::CacheManager;
use crate::config::CONFIG;
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
use tokio_stream::StreamExt;
use futures_util::stream::{TryStreamExt, IntoStream};

#[derive(Debug)]
pub enum DataSourceType {
    FileOnly,   // 完全缓存，只使用文件源
    NetworkOnly, // 无缓存，只使用网络源
    Mixed,      // 部分缓存，混合源
}

pub struct DataSourceManager {
    unit_pool: Arc<UnitPool>,
    storage: DataStorage,
    cache_manager: CacheManager,
}

impl DataSourceManager {
    pub fn new() -> Self {
        let unit_pool = Arc::new(UnitPool::new());
        let storage = DataStorage::new();
        let cache_manager = CacheManager::new(unit_pool.clone());

        // 启动定期清理缓存的
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
        // 1. 创建请求对象
        let data_request = DataRequest::new(&req)?;
        let url = data_request.get_url();
        let range = data_request.get_range();

        log_info!("Request", "处理请求: {} {}", url, range);

        // 2. 选择数据源策略
        let source_type = self.select_data_source(url, range).await?;
        log_info!("Source", "选择数据源: {:?}", source_type);

        // 3. 处理请求
        self.process_request_with_source(&data_request, source_type).await
    }

    async fn select_data_source(&self, url: &str, range: &str) -> Result<DataSourceType> {
        // 1. 检查网络状态
        if !self.check_network_available().await? {
            log_info!("Source", "网络不可用，使用文件源");
            return Ok(DataSourceType::FileOnly);
        }

        // 2. 加载缓存状态
        self.unit_pool.load_cache_state(url).await?;

        // 3. 检查缓存文件是否存在
        let cache_path = CONFIG.get_cache_file(url);
        if !tokio::fs::try_exists(&cache_path).await? {
            log_info!("Cache", "缓存文件不存在，使用网络源");
            return Ok(DataSourceType::NetworkOnly);
        }

        // 4. 检查缓存状态
        let cache_map = self.unit_pool.cache_map.read().await;
        if let Some(data_unit) = cache_map.get(&cache_path) {
            // 检查文件大小
            if let Ok(metadata) = tokio::fs::metadata(&cache_path).await {
                let file_size = metadata.len();
                if file_size == 0 {
                    log_info!("Cache", "缓存文件大小为0，使用网络源");
                    drop(cache_map);
                    return Ok(DataSourceType::NetworkOnly);
                }
            }

            // 检查是否完全缓存
            if data_unit.contains_range(0, data_unit.total_size.unwrap_or(0)) {
                log_info!("Cache", "完全缓存命中，使用文件源");
                drop(cache_map);
                return Ok(DataSourceType::FileOnly);
            }

            // 解析请求范围
            if let Ok((start, end)) = crate::utils::range::parse_range(range) {
                if data_unit.contains_range(start, end) {
                    log_info!("Cache", "请求范围完全缓存命中，使用文件源");
                    drop(cache_map);
                    return Ok(DataSourceType::FileOnly);
                } else if data_unit.partially_contains_range(start, end) {
                    log_info!("Cache", "请求范围部分缓存命中，使用混合源");
                    drop(cache_map);
                    return Ok(DataSourceType::Mixed);
                }
            }
        }
        drop(cache_map);

        log_info!("Cache", "缓存未命中，使用网络源");
        Ok(DataSourceType::NetworkOnly)
    }

    async fn process_request_with_source(&self, req: &DataRequest, source_type: DataSourceType) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();

        match source_type {
            DataSourceType::FileOnly => {
                self.process_file_source(url, range).await
            }
            DataSourceType::NetworkOnly => {
                self.process_network_source(url, range).await
            }
            DataSourceType::Mixed => {
                self.process_mixed_source(url, range).await
            }
        }
    }

    async fn process_file_source(&self, url: &str, range: &str) -> Result<Response<Body>> {
        // 1. 获取文件源
        let file_source = self.storage.get_file_source(url, range).await?;
        let stream = file_source.read_stream().await?;
        
        // 2. 创建响应流
        let (mut sender, body) = Body::channel();
        
        // 3. 处理流数据
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

        // 4. 构建响应
        let mut response = Response::builder().status(206);
        let headers = self.build_response_headers(range, false);
        
        for (key, value) in headers.iter() {
            response = response.header(key.as_str(), value);
        }

        response = response.header("Content-Range", format!("bytes {}", range));
        let response = response
            .body(body)
            .map_err(|e| ProxyError::Response(e.to_string()))?;
            
        Ok(response)
    }

    async fn process_network_source(&self, url: &str, range: &str) -> Result<Response<Body>> {
        // 1. 获取网络源
        let mut net_source = self.storage.get_net_source(url, range).await?;
        
        // 2. 下载数据
        match net_source.download_stream().await {
            Ok((response, total_size)) => {
                // 解析 range
                let start = if range.starts_with("bytes=") {
                    let parts: Vec<&str> = range[6..].split('-').collect();
                    parts[0].parse::<u64>().unwrap_or(0)
                } else {
                    0
                };
                
                // 3. 准备缓存
                let cache_path = CONFIG.get_cache_file(url);
                // 确保缓存目录存在
                if let Some(parent) = std::path::Path::new(&cache_path).parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let file = File::create(&cache_path).await?;
                let cache_writer = Arc::new(Mutex::new(Some(file)));
                
                // 设置文件总大小
                if total_size > 0 {
                    self.unit_pool.set_total_size(url, total_size).await?;
                }
                
                // 4. 创建流处理器
                let processor = StreamProcessor::new(
                    start,
                    url.to_string(),
                    self.unit_pool.clone(),
                );
                
                // 5. 处理响应体
                let stream = processor.process_stream(response.into_body(), cache_writer).await?;
                
                // 6. 构建响应
                let mut headers = self.build_response_headers(range, false);
                if total_size > 0 {
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
                }
                
                let response = Response::builder()
                    .status(206)
                    .header("Content-Range", headers.get("Content-Range").unwrap())
                    .header("Content-Type", headers.get("Content-Type").unwrap())
                    .header("Accept-Ranges", headers.get("Accept-Ranges").unwrap())
                    .body(stream)
                    .map_err(|e| ProxyError::Response(e.to_string()))?;
                Ok(response)
            }
            Err(e) => {
                log_error!("NetSource", "下载失败: {}", e);
                Err(ProxyError::Network(format!("下载失败: {}", e)))
            }
        }
    }

    async fn process_mixed_source(&self, url: &str, range: &str) -> Result<Response<Body>> {
        // 1. 解析请求范围
        let (start, end) = crate::utils::range::parse_range(range)?;
        
        // 2. 获取文件源和网络源
        let mut net_source = self.storage.get_net_source(url, range).await?;
        let file_source = self.storage.get_file_source(url, range).await?;
        
        // 3. 准备缓存文件（使用 OpenOptions 以追加模式打开）
        let cache_path = CONFIG.get_cache_file(url);
        // 确保缓存目录存在
        if let Some(parent) = std::path::Path::new(&cache_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&cache_path)
            .await?;
        let cache_writer = Arc::new(Mutex::new(Some(file)));
        
        // 4. 尝试网络下载
        match net_source.download_stream().await {
            Ok((response, total_size)) => {
                // 5. 创建流处理器
                let processor = StreamProcessor::new(
                    start,
                    url.to_string(),
                    self.unit_pool.clone(),
                );
                
                // 6. 获取并合并流
                let cached_stream = file_source.read_stream().await?;
                let network_stream = response.into_body();
                let merged_stream = processor
                    .merge_streams(cached_stream, network_stream, cache_writer)
                    .await?;
                
                // 7. 构建响应
                let mut headers = self.build_response_headers(range, true);
                headers.insert(
                    "Content-Range",
                    HeaderValue::from_str(&format!(
                        "bytes {}-{}/{}",
                        start,
                        end,
                        total_size
                    ))
                    .unwrap(),
                );
                
                let response = Response::builder()
                    .status(206)
                    .header("Content-Range", headers.get("Content-Range").unwrap())
                    .header("Content-Type", headers.get("Content-Type").unwrap())
                    .header("Accept-Ranges", headers.get("Accept-Ranges").unwrap())
                    .body(merged_stream)
                    .map_err(|e| ProxyError::Response(e.to_string()))?;
                Ok(response)
            }
            Err(_) => {
                // 8. 网络错误时尝试使用缓存
                log_info!("Source", "网络错误，尝试使用缓存数据");
                self.process_file_source(url, range).await
            }
        }
    }

    async fn check_network_available(&self) -> Result<bool> {
        let client = hyper::Client::new();
        let uri = hyper::Uri::from_static("http://www.baidu.com");

        match client.get(uri).await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(e) => {
                log_error!("Network", "网络连接检查失败: {}", e);
                Ok(false)
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