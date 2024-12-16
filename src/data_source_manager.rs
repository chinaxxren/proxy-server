use crate::cache::unit_pool::UnitPool;
use crate::cache::CacheManager;
use crate::config::CONFIG;
use crate::data_request::DataRequest;
use crate::data_storage::DataStorage;
use crate::stream_processor::StreamProcessor;
use crate::utils::error::{ProxyError, Result};
use crate::{log_error, log_info};
use futures_util::stream::{IntoStream, TryStreamExt};
use hyper::header::{HeaderMap, HeaderValue};
use hyper::{Body, Request, Response};
use std::sync::Arc;
use tokio::fs::File;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use tokio_stream::StreamExt;

#[derive(Debug)]
pub enum DataSourceType {
    FileOnly,    // 完全缓存，只使用文件源
    NetworkOnly, // 无缓存，只使用网络源
    Mixed,       // 部分缓存，混合源
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

        // 启动定期清理缓存
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
        self.process_request_with_source(&data_request, source_type)
            .await
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
            log_info!("Cache", "找到缓存单元: {:?}", data_unit);
            
            // 检查文件大小
            if let Ok(metadata) = tokio::fs::metadata(&cache_path).await {
                let file_size = metadata.len();
                if file_size == 0 {
                    log_info!("Cache", "缓存文件大小为0，使用网络源");
                    drop(cache_map);
                    return Ok(DataSourceType::NetworkOnly);
                }

                // 解析请求范围
                let (start, end) = crate::utils::range::parse_range(range)?;
                let actual_end = if end == 0 || end == u64::MAX {
                    file_size - 1
                } else {
                    end
                };
                log_info!("Cache", "解析请求范围: {}-{} (实际结束位置: {})", start, end, actual_end);

                if self.check_cache_coverage(&data_unit.ranges, start, actual_end) {
                    log_info!("Cache", "请求范围完全缓存命中，使用文件源");
                    drop(cache_map);
                    return Ok(DataSourceType::FileOnly);
                } else {
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

    async fn process_request_with_source(
        &self,
        req: &DataRequest,
        source_type: DataSourceType,
    ) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();

        match source_type {
            DataSourceType::FileOnly => self.process_file_source(url, range).await,
            DataSourceType::NetworkOnly => self.process_network_source(url, range).await,
            DataSourceType::Mixed => self.process_mixed_source(url, range).await,
        }
    }

    async fn process_file_source(&self, url: &str, range: &str) -> Result<Response<Body>> {
        // 1. 获取文件源
        let file_source = self.storage.get_file_source(url, range).await?;

        // 2. 获取文件大小
        let cache_path = CONFIG.get_cache_file(url);
        let file_size = tokio::fs::metadata(&cache_path).await?.len();

        // 3. 解析范围
        let (start, end) = crate::utils::range::parse_range(range)?;
        let actual_end = if end == u64::MAX || end == 0 {
            file_size - 1
        } else {
            std::cmp::min(end, file_size - 1)
        };

        // 计算实际的数据长度
        let content_length = actual_end - start + 1;

        // 4. 获取文件流
        let stream = file_source.read_stream().await?;

        // 5. 创建响应
        let (mut sender, body) = Body::channel();

        // 6. 处理流数据
        let mut total_sent = 0u64;
        tokio::spawn(async move {
            let mut stream = Box::pin(stream);
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(mut data) => {
                        let remaining = content_length - total_sent;
                        if remaining == 0 {
                            break;
                        }

                        // 如果当前数据块大于剩余需要发送的数据量，只发送需要的部分
                        if (total_sent + data.len() as u64) > content_length {
                            let needed = remaining as usize;
                            data.truncate(needed);
                        }

                        total_sent += data.len() as u64;
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

        // 7. 构建响应
        let mut response = Response::builder().status(206);
        let headers = self.build_response_headers(range, false);

        for (key, value) in headers.iter() {
            response = response.header(key.as_str(), value);
        }

        // 添加正确格式的 Content-Range 头
        response = response.header(
            "Content-Range",
            format!("bytes {}-{}/{}", start, actual_end, file_size),
        );

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
                    if range.ends_with('-') { total_size - 1 } else { range.split('-').last().unwrap().parse::<u64>().unwrap() },
                    url.to_string(),
                    self.unit_pool.clone()
                );

                // 5. 处理响应体
                let stream = processor
                    .process_stream(response.into_body(), cache_writer)
                    .await?;

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
        log_info!("Mixed", "开始处理混合源请求: {}-{} (原始range: {})", start, end, range);

        // 2. 获取文件源
        let file_source = self.storage.get_file_source(url, range).await?;

        // 3. 检查缓存状态
        let cache_path = CONFIG.get_cache_file(url);
        let cache_map = self.unit_pool.cache_map.read().await;
        if let Some(data_unit) = cache_map.get(&cache_path) {
            log_info!("Mixed", "当前缓存状态: {:?}", data_unit);
            log_info!("Mixed", "缓存文件路径: {}", cache_path);
            
            if self.check_cache_coverage(&data_unit.ranges, start, end) {
                log_info!("Mixed", "发现完全缓存命中，切换到文件源");
                drop(cache_map);
                return self.process_file_source(url, range).await;
            }
        }
        drop(cache_map);

        // 4. 准备缓存文件
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
        log_info!("Mixed", "准备缓存文件: {}", cache_path);

        // 5. 获取网络源并尝试下载
        let mut net_source = self.storage.get_net_source(url, range).await?;
        match net_source.download_stream().await {
            Ok((response, total_size)) => {
                log_info!("Mixed", "网络请求成功，文件总大小: {}", total_size);
                
                // 6. 创建流处理器
                let processor = StreamProcessor::new(
                    start,
                    end,
                    url.to_string(),
                    self.unit_pool.clone()
                );

                // 7. 获取并合并流
                let cached_stream = file_source.read_stream().await?;
                let network_stream = response.into_body();
                log_info!("Mixed", "开始合并缓存流和网络流");
                let merged_stream = processor
                    .merge_streams(cached_stream, network_stream, cache_writer)
                    .await?;

                // 8. 构建响应
                let mut headers = self.build_response_headers(range, true);
                headers.insert(
                    "Content-Range",
                    HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end, total_size))
                        .unwrap(),
                );
                log_info!("Mixed", "构建响应头: Content-Range: bytes {}-{}/{}", start, end, total_size);

                let response = Response::builder()
                    .status(206)
                    .header("Content-Range", headers.get("Content-Range").unwrap())
                    .header("Content-Type", headers.get("Content-Type").unwrap())
                    .header("Accept-Ranges", headers.get("Accept-Ranges").unwrap())
                    .body(merged_stream)
                    .map_err(|e| ProxyError::Response(e.to_string()))?;

                Ok(response)
            }
            Err(e) => {
                log_error!("Mixed", "网络请求失败: {}, 尝试使用缓存数据", e);
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

    fn check_cache_coverage(&self, ranges: &[(u64, u64)], start: u64, end: u64) -> bool {
        log_info!("Cache", "开始检查缓存覆盖 - 请范围: {}-{}", start, end);
        
        // 首先对范围进行排序
        let mut sorted_ranges = ranges.to_vec();
        sorted_ranges.sort_by_key(|&(start, _)| start);
        log_info!("Cache", "已缓存的范围: {:?}", sorted_ranges);

        // 检查每个缓存范围是否完全覆盖请求范围
        for &(cache_start, cache_end) in &sorted_ranges {
            if cache_start <= start && cache_end >= end {
                log_info!("Cache", "找到完全覆盖: 缓存范围 {}-{} 覆盖请求范围 {}-{}", 
                         cache_start, cache_end, start, end);
                return true;
            }
        }

        // 如果没有单个范围完全覆盖，则检查多个范围的组合覆盖
        let mut current_pos = start;
        for &(cache_start, cache_end) in &sorted_ranges {
            // 如果当前缓存范围在目标位置之前，跳过
            if cache_end < current_pos {
                continue;
            }
            
            // 如果当前缓存范围与目标位置有重叠
            if cache_start <= current_pos {
                // 更新当前位置到缓存范围的结束位置+1
                current_pos = cache_end + 1;
                log_info!("Cache", "部分覆盖: 更新当前位置到 {}", current_pos);
                
                // 如果已经覆盖了整个请求范围
                if current_pos > end {
                    log_info!("Cache", "通过多个范围完全覆盖");
                    return true;
                }
            } else {
                // 如果出现了空隙，说明不连续
                break;
            }
        }

        let result = current_pos > end;
        log_info!("Cache", "最终检查结果: {} (当前位置: {})", result, current_pos);
        result
    }
}

impl Default for DataSourceManager {
    fn default() -> Self {
        Self::new()
    }
}
