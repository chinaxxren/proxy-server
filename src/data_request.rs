use crate::log_info;
use crate::utils::error::{ProxyError, Result};
use hyper::{
    header::{HeaderMap, HeaderValue, RANGE},
    Request,
};
use url::Url;

pub struct DataRequest {
    url: String,
    range: String,
    headers: HeaderMap,
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
                proxy_path.to_string()
            } else {
                // 如果不是 /proxy/ 格式，尝试查询参数
                let uri = req.uri().to_string();
                let parsed_url = Url::parse(&uri)
                    .map_err(|_| ProxyError::Request("无效的请求URL".to_string()))?;
                
                let proxy_param = parsed_url.query_pairs()
                    .find(|(key, _)| key == "proxy")
                    .map(|(_, value)| value.into_owned())
                    .ok_or_else(|| ProxyError::Request("缺少proxy参数".to_string()))?;

                urlencoding::decode(&proxy_param)
                    .map_err(|_| ProxyError::Request("无效的proxy参数编码".to_string()))?
                    .into_owned()
            }
        };

        let range = if let Some(range_header) = req.headers().get(RANGE) {
            range_header.to_str()?.to_string()
        } else {
            "bytes=0-".to_string()  // 默认请求完整文件
        };

        log_info!("Request", "url: {}", url);
        log_info!("Request", "key: range, value: {}", range);

        Ok(Self {
            url,
            range,
            headers: req.headers().clone(),
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
}
