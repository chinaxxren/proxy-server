use crate::utils::error::{ProxyError, Result};
use hyper::{
    header::{HeaderMap, HeaderValue, RANGE},
    Request,
};

pub struct DataRequest {
    url: String,
    range: String,
    headers: HeaderMap,
}

impl DataRequest {
    pub fn new(req: &Request<hyper::Body>) -> Result<Self> {
        let original_url = original_url(req)?;

        let range = req
            .headers()
            .get(RANGE)
            .and_then(|r| r.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !range.is_empty() && !range.starts_with("bytes=") {
            return Err(ProxyError::Range("无效的Range格式".to_string()));
        }

        let mut headers = HeaderMap::new();
        for (key, value) in req.headers() {
            if let Ok(v) = HeaderValue::from_bytes(value.as_bytes()) {
                println!("key: {}, value: {}", key, value);
                headers.insert(key, v);
            }
        }

        Ok(Self {
            url: original_url,
            range,
            headers,
        })
    }

    pub fn new_request_with_range(url: &str, range: &str) -> Request<hyper::Body> {
        let mut builder = Request::builder().method("GET").uri(url);

        if !range.is_empty() {
            if let Ok(value) = HeaderValue::from_str(range) {
                builder = builder.header(RANGE, value);
            }
        }

        builder = builder
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .header("Accept", "*/*");

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
}

fn original_url(req: &Request<hyper::Body>) -> Result<String> {
    let original_url = req.headers().get("X-Original-Url");
    // 获取 X-Original-Url 头部
    match original_url {
        Some(value) => {
            Ok(value
                .to_str()
                .map_err(|_| ProxyError::Request("X-Original-Url 头部 格式错误".to_string()))?
                .to_string())
        }
        None => {
            return Err(ProxyError::Request("无效的URL".to_string()));
        }
    }
}
