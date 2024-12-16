use crate::utils::error::{Result, ProxyError};
use crate::{log_info};
use hyper::{Client, Request, Body, Method};
use hyper::header::{CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use hyper_tls::HttpsConnector;

pub struct SizeChecker {
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
}

impl SizeChecker {
    pub fn new() -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, Body>(https);
        Self { client }
    }

    pub async fn check_file_size(&self, url: &str) -> Result<u64> {
        // 1. 尝试 HEAD 请求
        if let Ok(size) = self.check_size_by_head(url).await {
            log_info!("SizeChecker", "通过 HEAD 请求获取文件大小: {}", size);
            return Ok(size);
        }

        // 2. 尝试 Range 请求
        if let Ok(size) = self.check_size_by_range(url).await {
            log_info!("SizeChecker", "通过 Range 请求获取文件大小: {}", size);
            return Ok(size);
        }

        // 3. 尝试普通 GET 请求
        if let Ok(size) = self.check_size_by_get(url).await {
            log_info!("SizeChecker", "通过 GET 请求获取文件大小: {}", size);
            return Ok(size);
        }

        Err(ProxyError::Request("无法获取文件大小".to_string()))
    }

    async fn check_size_by_head(&self, url: &str) -> Result<u64> {
        let req = Request::builder()
            .method(Method::HEAD)
            .uri(url)
            .body(Body::empty())?;

        let resp = self.client.request(req).await?;
        if let Some(len) = resp.headers().get(CONTENT_LENGTH) {
            if let Ok(size) = len.to_str()?.parse::<u64>() {
                return Ok(size);
            }
        }
        Err(ProxyError::Request("HEAD 请求未返回文件大小".to_string()))
    }

    async fn check_size_by_range(&self, url: &str) -> Result<u64> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header(RANGE, "bytes=0-0")
            .body(Body::empty())?;

        let resp = self.client.request(req).await?;
        if let Some(range) = resp.headers().get(CONTENT_RANGE) {
            let range_str = range.to_str()?;
            if let Some(size_str) = range_str.split('/').last() {
                if let Ok(size) = size_str.parse::<u64>() {
                    return Ok(size);
                }
            }
        }
        Err(ProxyError::Request("Range 请求未返回文件大小".to_string()))
    }

    async fn check_size_by_get(&self, url: &str) -> Result<u64> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .body(Body::empty())?;

        let resp = self.client.request(req).await?;
        if let Some(len) = resp.headers().get(CONTENT_LENGTH) {
            if let Ok(size) = len.to_str()?.parse::<u64>() {
                return Ok(size);
            }
        }
        Err(ProxyError::Request("GET 请求未返回文件大小".to_string()))
    }
} 