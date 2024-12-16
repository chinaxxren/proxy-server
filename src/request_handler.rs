use crate::data_request::DataRequest;
use crate::data_source_manager::DataSourceManager;
use crate::hls::{DefaultHlsHandler, HlsHandler};
use crate::utils::error::Result;
use hyper::{Body, Request, Response};
use std::sync::Arc;

pub struct RequestHandler {
    source_manager: Arc<DataSourceManager>,
    hls_handler: Arc<DefaultHlsHandler>,
}

impl RequestHandler {
    pub fn new(source_manager: Arc<DataSourceManager>, hls_handler: Arc<DefaultHlsHandler>) -> Self {
        Self {
            source_manager,
            hls_handler,
        }
    }
    
    pub async fn handle_request(&self, req: Request<Body>) -> Result<Response<Body>> {
        let data_request = DataRequest::new(&req)?;
        
        match data_request.get_type() {
            crate::data_request::RequestType::M3u8 => {
                // 处理 m3u8 请求
                let content = self.hls_handler.handle_m3u8(data_request.get_url()).await?;
                Ok(Response::new(Body::from(content)))
            }
            crate::data_request::RequestType::Segment => {
                // 处理分片请求
                let data = self.hls_handler
                    .handle_segment(data_request.get_url(), Some(data_request.get_range().to_string()))
                    .await?;
                Ok(Response::new(Body::from(data)))
            }
            _ => {
                // 处理普通请求
                self.source_manager.process_request(&data_request).await
            }
        }
    }
}
