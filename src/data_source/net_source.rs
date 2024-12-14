use crate::utils::error::ProxyError;
use hyper::{Client, Response};
use crate::data_request::DataRequest;

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

    pub async fn download_data(&self) -> Result<Vec<u8>, ProxyError> {
        let data_request = DataRequest::new_request_with_range(&self.url, &self.range);
        let resp: Response<hyper::Body> = self.client.request(data_request).await?;

        if !resp.status().is_success() {
            return Err(ProxyError::Data("HTTP request failed".to_string()));
        }

        let bytes = hyper::body::to_bytes(resp.into_body()).await?;
        Ok(bytes.to_vec())
    }
}
