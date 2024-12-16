use crate::data_request::DataRequest;
use crate::utils::error::Result;
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
        let client = hyper::Client::builder().build::<_, hyper::Body>(https);
        
        let req = DataRequest::new_request_with_range(&self.url, &self.range);
        let resp = client.request(req).await?;
        
        let content_length = resp.headers()
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        
        Ok((resp, content_length))
    }
}
