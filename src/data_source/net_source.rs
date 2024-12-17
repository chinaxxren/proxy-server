use std::time::Duration;

use crate::log_info;
use crate::{data_request::DataRequest, utils::error::ProxyError};
use crate::utils::error::Result;
use hyper::client::HttpConnector;
use hyper::{Body, Response};
use hyper_tls::HttpsConnector;

#[derive(Debug, Clone)]
pub struct NetSource {
    pub url: String,
    pub range: String,
}

impl NetSource {
    pub fn new(url: &str, range: &str) -> Self {
        Self {
            url: url.to_string(),
            range: range.to_string(),
        }
    }
    
    pub async fn download_stream(&self) -> Result<(Response<Body>, u64)> {
        let https = HttpsConnector::new();
        let client = hyper::Client::builder()
        .pool_idle_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(0)
        .build::<_, hyper::Body>(https);
        
        let mut retries = 3;
        while retries > 0 {
            match self.try_download(&client).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    retries -= 1;
                    if retries == 0 {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
        
        Err(ProxyError::Request("Max retries reached".into()))
    }

    async fn try_download(&self, client: &hyper::Client<HttpsConnector<HttpConnector>>) -> Result<(Response<Body>, u64)> {
        let req = DataRequest::new_request_with_range(&self.url, &self.range);
        let resp = client.request(req).await?;
        
        // 验证响应状态码
        if !resp.status().is_success() {
            return Err(ProxyError::Request(format!("Invalid response status: {}", resp.status())));
        }
    
        // 获取并验证 Content-Length
        let content_length = match resp.headers().get(hyper::header::CONTENT_LENGTH) {
            Some(len) => len.to_str()
                .map_err(|_| ProxyError::Request("Invalid content length header".into()))?
                .parse::<u64>()
                .map_err(|_| ProxyError::Request("Invalid content length value".into()))?,
            None => return Err(ProxyError::Request("Missing content length header".into()))
        };
    
        // 验证 Content-Range
        if let Some(range) = resp.headers().get(hyper::header::CONTENT_RANGE) {
            let range_str = range.to_str()
                .map_err(|_| ProxyError::Request("Invalid content range header".into()))?;
            // 可以添加进一步的范围验证
            log_info!("Request", "Range header: {}", range_str);
        }
    
        // 创建新的响应，使用可能更稳定的 Body 实现
        let (parts, body) = resp.into_parts();
        let body = hyper::Body::wrap_stream(body);
        let resp = Response::from_parts(parts, body);
    
        Ok((resp, content_length))
    }
}
