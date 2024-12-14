use crate::utils::error::{Result, ProxyError};
use crate::data_request::DataRequest;
use hyper::{Client, Response};

pub struct NetSource {
    url: String,
    range: String,
    client: Client<hyper::client::HttpConnector>,
}

impl NetSource {
    pub fn new(url: &str, range: &str) -> Self {
        Self {
            url: url.to_string(),
            range: range.to_string(),
            client: Client::new(),
        }
    }

    pub async fn download_data(&self) -> Result<Vec<u8>> {
        let data_request = DataRequest::new_request_with_range(&self.url, &self.range);
        let resp: Response<hyper::Body> = self.client.request(data_request).await?;

        if !resp.status().is_success() {
            // 打印错误信息？
            println!("HTTP request failed: {}", resp.status());
            println!("HTTP request failed: {:#?}", resp.headers());
            println!("HTTP request failed: {:#?}", resp.body());
            return Err(ProxyError::Request("HTTP request failed".to_string()));
        }

        let bytes = hyper::body::to_bytes(resp.into_body()).await?;
        Ok(bytes.to_vec())
    }
}
