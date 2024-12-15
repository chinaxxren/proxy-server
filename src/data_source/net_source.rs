use crate::data_request::DataRequest;
use crate::utils::error::{ProxyError, Result};
use crate::{log_error, log_info};
use hyper::client::HttpConnector;
use hyper::header::{CONTENT_LENGTH, CONTENT_RANGE};
use hyper::{Body, Client, Response};
use hyper_tls::HttpsConnector;

pub struct NetSource {
    url: String,
    range: String,
    client: Client<HttpsConnector<HttpConnector>>,
}

impl NetSource {
    pub fn new(url: &str, range: &str) -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);

        Self {
            url: url.to_string(),
            range: range.to_string(),
            client,
        }
    }

    pub async fn download_stream(&mut self) -> Result<(Response<Body>, u64)> {
        let data_request = DataRequest::new_request_with_range(&self.url, &self.range);

        match self.client.request(data_request).await {
            Ok(resp) => {
                log_info!("NetSource", "resp status: {:#?}", resp.status());

                if !resp.status().is_success() {
                    log_error!("NetSource", "HTTP request failed: {}", resp.status());
                    log_error!(
                        "NetSource",
                        "HTTP request failed headers: {:#?}",
                        resp.headers()
                    );
                    return Err(ProxyError::Request("HTTP request failed".to_string()));
                }

                // 从响应头中获取文件总大小
                let total_size = if let Some(content_range) = resp.headers().get(CONTENT_RANGE) {
                    if let Ok(range_str) = content_range.to_str() {
                        if let Some(size_str) = range_str.split('/').last() {
                            size_str.parse::<u64>().unwrap_or(0)
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else if let Some(content_length) = resp.headers().get(CONTENT_LENGTH) {
                    content_length
                        .to_str()
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0)
                } else {
                    0
                };

                // 如果是无限长度的请求，更新range字符串
                if self.range.ends_with('-') && total_size > 0 {
                    let start = self.range[6..self.range.len() - 1]
                        .parse::<u64>()
                        .unwrap_or(0);
                    self.range = format!("bytes={}-{}", start, total_size - 1);
                }

                Ok((resp, total_size))
            }
            Err(e) => {
                log_error!("NetSource", "HTTP request failed: {}", e);
                Err(ProxyError::Request("HTTP request failed".to_string()))
            }
        }
    }
}
