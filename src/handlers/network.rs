use hyper::{Body, Response, HeaderMap};
use crate::data_source::NetSource;
use crate::utils::error::Result;
use crate::log_info;

pub struct NetworkHandler;

impl NetworkHandler {
    pub fn new() -> Self {
        Self
    }

    pub async fn fetch(&self, url: &str, range: &str) -> Result<(Response<Body>, u64, u64)> {
        let net_source = NetSource::new(url, range);
        let (resp, content_length) = net_source.download_stream().await?;
        log_info!("Cache", "网络响应成功，内容长度: {}", content_length);

        // 获取文件总大小
        let total_size = if let Some(range) = resp.headers().get(hyper::header::CONTENT_RANGE) {
            if let Ok(range_str) = range.to_str() {
                if let Some(total) = range_str.split('/').last() {
                    total.parse::<u64>().unwrap_or(0)
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            0
        };

        Ok((resp, content_length, total_size))
    }

    pub fn extract_headers(&self, resp: &Response<Body>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (key, value) in resp.headers().iter() {
            if key != hyper::header::CONTENT_RANGE && key != hyper::header::CONTENT_LENGTH {
                headers.insert(key, value.clone());
            }
        }
        headers
    }
} 