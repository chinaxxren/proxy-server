use crate::utils::error::{ProxyError, Result};
use hyper::{header::{HeaderMap, HeaderValue, RANGE}, Request};

pub struct DataRequest {
    url: String,
    range: String,
    headers: HeaderMap,
}

impl DataRequest {
    pub fn new(req: &Request<hyper::Body>) -> Result<Self> {
        let url = req.uri().to_string();
        let range = req.headers()
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
                headers.insert(key, v);
            }
        }

        Ok(Self { 
            url, 
            range,
            headers 
        })
    }

    pub fn new_request_with_range(url: &str, range: &str) -> Request<hyper::Body> {
        let mut builder = Request::builder()
            .method("GET")
            .uri(url);

        if !range.is_empty() {
            if let Ok(value) = HeaderValue::from_str(range) {
                builder = builder.header(RANGE, value);
            }
        }

        builder = builder
            .header("User-Agent", "KTVHC-Proxy/1.0")
            .header("Accept", "*/*");

        builder.body(hyper::Body::empty())
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
