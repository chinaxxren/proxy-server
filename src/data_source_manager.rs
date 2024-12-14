use crate::cache::unit_pool::UnitPool;
use crate::cache::CacheManager;
use crate::data_request::DataRequest;
use crate::data_storage::DataStorage;
use crate::utils::error::{ProxyError, Result};
use crate::utils::range::parse_range;
use hyper::header::{HeaderMap, HeaderValue};
use hyper::{Body, Request, Response};
use std::sync::Arc;

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
            println!("完全缓存");
            Ok(DataSourceType::FileOnly)
        } else if self.unit_pool.is_partially_cached(url, range).await? {
            println!("部分缓存");
            Ok(DataSourceType::Mixed)
        } else {
            println!("网络缓存");
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
                println!("网络连接检查失败");
                Ok(false)
            }
        }
    }

    async fn merge_data(
        &self,
        cached_data: Vec<u8>,
        downloaded_data: Vec<u8>,
        range: &str,
    ) -> Result<Vec<u8>> {
        let (start, end) = parse_range(range)?;
        let total_size = end - start + 1;
        let mut merged_data = vec![0; total_size as usize];

        // 复制缓存数据
        if !cached_data.is_empty() {
            merged_data[..cached_data.len()].copy_from_slice(&cached_data);
        }

        // 复制下载的数据
        if !downloaded_data.is_empty() {
            let offset = cached_data.len();
            merged_data[offset..].copy_from_slice(&downloaded_data);
        }

        Ok(merged_data)
    }

    async fn _handle_network_error(&self, url: &str, range: &str) -> Result<Vec<u8>> {
        // 尝试从缓存读取部分数据
        if let Ok(cached_data) = self.storage.get_cached_data(url, range).await {
            return Ok(cached_data);
        }

        // 如果无法从缓存读取，重试网络请求
        for retry in 0..3 {
            if let Ok(data) = self._retry_download(url, range).await {
                return Ok(data);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1 << retry)).await;
        }

        Err(ProxyError::Network("无法获取数据".to_string()))
    }

    async fn _retry_download(&self, url: &str, range: &str) -> Result<Vec<u8>> {
        let net_source = self.storage.get_net_source(url, range).await?;
        net_source.download_data().await
    }

    async fn validate_data(&self, data: &[u8], expected_size: u64) -> Result<()> {
        // 检查数据大小
        if data.len() as u64 != expected_size {
            return Err(ProxyError::Data("数据大小不匹配".to_string()));
        }

        // 检查数据完整性
        if !self.verify_data_integrity(data).await? {
            return Err(ProxyError::Data("数据完整性验证失败".to_string()));
        }

        Ok(())
    }

    async fn verify_data_integrity(&self, data: &[u8]) -> Result<bool> {
        // 实现基本的数据完整性检查
        if data.is_empty() {
            return Err(ProxyError::Data("数据为空".to_string()));
        }

        // 检查数据是否全为零
        let zero_chunk = data.chunks(1024).any(|chunk| chunk.iter().all(|&x| x == 0));
        if zero_chunk {
            return Err(ProxyError::Data("数据包含无效块".to_string()));
        }

        // 检查数据长度是否合理
        if data.len() > 1024 * 1024 * 100 {
            // 100MB
            return Err(ProxyError::Data("数据大小超出限制".to_string()));
        }

        Ok(true)
    }

    async fn process_request_with_cache(&self, req: &DataRequest) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();

        println!("url: {}", url);
        println!("range: {}", range);

        match self.select_data_source(url, range).await? {
            // 文件缓存
            DataSourceType::FileOnly => {
                // 完整缓存处理
                let file_source = self.storage.get_file_source(url, range).await?;
                let data = file_source.read_data().await?;

                let headers = self.build_response_headers(range, false);
                let mut response = Response::builder().status(206);

                // 添加所有响应头
                for (key, value) in headers.iter() {
                    response = response.header(key.as_str(), value);
                }

                response = response.header("Content-Range", format!("bytes {}", range));
                let response = response
                    .body(Body::from(data))
                    .map_err(|e| ProxyError::Response(e.to_string()))?;
                Ok(response)
            }
            // 混合缓存
            DataSourceType::Mixed => {
                // 部分缓存处理
                let cached_data = self.storage.get_cached_data(url, range).await?;
                let net_source = self.storage.get_net_source(url, range).await?;

                match net_source.download_data().await {
                    Ok(downloaded_data) => {
                        let merged_data =
                            self.merge_data(cached_data, downloaded_data, range).await?;
                        self.unit_pool.write_cache(url, range, &merged_data).await?;
                        self.cache_manager.merge_files_if_needed(url).await?;

                        let headers = self.build_response_headers(range, true);
                        let mut response = Response::builder().status(206);

                        for (key, value) in headers.iter() {
                            response = response.header(key.as_str(), value);
                        }

                        response = response.header("Content-Range", format!("bytes {}", range));
                        let response = response
                            .body(Body::from(merged_data))
                            .map_err(|e| ProxyError::Response(e.to_string()))?;
                        Ok(response)
                    }
                    Err(_) => {
                        // 网络错误时尝试使用缓存数据
                        let headers = self.build_response_headers(range, true);
                        let mut response = Response::builder().status(206);

                        for (key, value) in headers.iter() {
                            response = response.header(key.as_str(), value);
                        }

                        response = response
                            .header("Content-Range", format!("bytes {}", range))
                            .header("X-Cache-Status", "partial");

                        let response = response
                            .body(Body::from(cached_data))
                            .map_err(|e| ProxyError::Response(e.to_string()))?;
                        Ok(response)
                    }
                }
            }

            // 网络缓存
            DataSourceType::NetworkOnly => {
                // 无缓存处理
                let net_source = self.storage.get_net_source(url, range).await?;
                match net_source.download_data().await {
                    Ok(data) => {
                        self.unit_pool.write_cache(url, range, &data).await?;
                        let (_, end) = parse_range(range)?;
                        self.validate_data(&data, end + 1).await?;

                        let headers = self.build_response_headers(range, false);
                        let mut response = Response::builder().status(200);

                        for (key, value) in headers.iter() {
                            response = response.header(key.as_str(), value);
                        }

                        let response = response
                            .body(Body::from(data))
                            .map_err(|e| ProxyError::Response(e.to_string()))?;
                        Ok(response)
                    }
                    Err(e) => Err(ProxyError::Network(format!("下载失败: {}", e))),
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
