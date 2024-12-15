use crate::data_request::DataRequest;
use crate::utils::error::{ProxyError, Result};
use crate::{log_info, log_error};
use hyper::client::HttpConnector;
use hyper::{Client, Response};
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

    pub async fn download_data(&mut self) -> Result<Vec<u8>> {
        let data_request = DataRequest::new_request_with_range(&self.url, &self.range);
        log_info!("NetSource", "data_request headers: {:#?}", data_request.headers());

        let resp: std::result::Result<Response<hyper::Body>, hyper::Error> =
            self.client.request(data_request).await;

        match resp {
            Ok(resp) => {
                log_info!("NetSource", "resp status: {:#?}", resp.status());
                if !resp.status().is_success() {
                    log_error!("NetSource", "HTTP request failed: {}", resp.status());
                    log_error!("NetSource", "HTTP request failed headers: {:#?}", resp.headers());
                    return Err(ProxyError::Request("HTTP request failed".to_string()));
                }

                let bytes = hyper::body::to_bytes(resp.into_body()).await?;
                log_info!("NetSource", "downloaded bytes: {}", bytes.len());
                Ok(bytes.to_vec())
            }
            Err(e) => {
                log_error!("NetSource", "HTTP request failed: {}", e);
                Err(ProxyError::Request("HTTP request failed".to_string()))
            }
        }
    }
}
