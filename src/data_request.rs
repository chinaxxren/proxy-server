use crate::log_info;
use crate::utils::error::{ProxyError, Result};
use hyper::{
    header::{HeaderMap, HeaderValue, RANGE},
    Request,
};
use url::Url;
use urlencoding;

#[derive(Debug, Clone)]
pub enum RequestType {
    Normal,
    M3u8,
    Segment,
}

#[derive(Debug, Clone)]
pub struct DataRequest {
    pub url: String,
    pub range: String,
    pub headers: HeaderMap,
    pub request_type: RequestType,
}

impl DataRequest {
    pub fn new(req: &Request<hyper::Body>) -> Result<Self> {
        log_info!("Request", "req: {}", req.uri());
        
        let url = if let Some(original_url) = req.headers().get("X-Original-Url") {
            original_url.to_str()?.to_string()
        } else {
            let path = req.uri().path();
            
            // 检查是否是 /proxy/ 格式
            if let Some(proxy_path) = path.strip_prefix("/proxy/") {
                // 处理可能存在的多重 /proxy/ 前缀
                let mut clean_url = proxy_path.to_string();
                while let Some(idx) = clean_url.find("/proxy/") {
                    clean_url = clean_url[idx + 7..].to_string();
                }
                
                // 解码 URL
                urlencoding::decode(&clean_url)
                    .map_err(|e| ProxyError::Request(format!("URL 解码失败: {}", e)))?
                    .into_owned()
            } else {
                // 如果不是 /proxy/ 格式，尝试查询参数
                let uri = req.uri().to_string();
                let parsed_url = Url::parse(&uri)
                    .map_err(|_| ProxyError::Request("无效的请求URL".to_string()))?;
                
                parsed_url.to_string()
            }
        };

        log_info!("Request", "url: {}", url);
        
        // 获取 Range 头
        let range = if let Some(range_header) = req.headers().get(RANGE) {
            range_header.to_str()?.to_string()
        } else {
            "bytes=0-".to_string()
        };
        
        log_info!("Request", "key: range, value: {}", range);
        
        // 确定请求类型
        let request_type = if url.ends_with(".m3u8") {
            log_info!("Request", "type: M3u8");
            RequestType::M3u8
        } else if url.ends_with(".ts") {
            log_info!("Request", "type: Segment");
            RequestType::Segment
        } else {
            log_info!("Request", "type: Normal");
            RequestType::Normal
        };
        
        Ok(Self {
            url,
            range,
            headers: req.headers().clone(),
            request_type,
        })
    }

    pub fn new_request_with_range(url: &str, range: &str) -> Request<hyper::Body> {
        let mut builder = Request::builder().method("GET").uri(url);

        // 总是添加 Range 头，因为现在我们总是有一个值
        if let Ok(value) = HeaderValue::from_str(range) {
            builder = builder.header(RANGE, value);
            log_info!("Request", "Range header: {}", range);
        }

        builder = builder
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .header("Accept", "*/*")
            .header("Connection", "keep-alive");

        builder
            .body(hyper::Body::empty())
            .unwrap_or_else(|_| Request::new(hyper::Body::empty()))
    }

    pub fn get_url(&self) -> &str {
        &self.url
    }

    pub fn get_range(&self) -> &str {
        &self.range
    }

    pub fn get_headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn get_type(&self) -> &RequestType {
        &self.request_type
    }
}
