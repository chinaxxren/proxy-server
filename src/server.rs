use crate::data_source_manager::DataSourceManager;
use crate::hls::DefaultHlsHandler;
use crate::request_handler::RequestHandler;
use crate::utils::error::Result;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use crate::log_info;

pub struct ProxyServer {
    port: u16,
    handler: Arc<RequestHandler>,
}

impl ProxyServer {
    pub fn new(port: u16, cache_dir: &str) -> Self {
        let cache_dir = PathBuf::from(cache_dir);
        
        // 创建数据源管理器
        let source_manager = Arc::new(DataSourceManager::new(cache_dir.clone()));
        
        // 创建 HLS 处理器
        let hls_handler = Arc::new(DefaultHlsHandler::new(cache_dir.clone(), source_manager.clone()));
        
        // 创建请求处理器
        let handler = Arc::new(RequestHandler::new(source_manager, hls_handler));
        
        Self {
            port,
            handler,
        }
    }
    
    pub async fn start(&self) -> Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        
        let handler = self.handler.clone();
        let make_svc = make_service_fn(move |_conn| {
            let handler = handler.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let handler = handler.clone();
                    async move {
                        match handler.handle_request(req).await {
                            Ok(response) => Ok::<_, Infallible>(response),
                            Err(e) => {
                                let error_message = format!("Error: {}", e);
                                Ok(hyper::Response::builder()
                                    .status(500)
                                    .body(hyper::Body::from(error_message))
                                    .unwrap())
                            }
                        }
                    }
                }))
            }
        });
        
        let server = Server::bind(&addr).serve(make_svc);
        log_info!("Server", "代理服务器正在运行在 http://{}", addr);
        
        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
        
        Ok(())
    }
}

pub async fn run_server(port: u16, cache_dir: &str) -> Result<()> {
    let server = ProxyServer::new(port, cache_dir);
    server.start().await
}