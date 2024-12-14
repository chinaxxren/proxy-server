use hyper::{Request, Response};
use crate::utils::error::{ProxyError, Result};
use crate::data_source_manager::DataSourceManager;
use crate::data_request::DataRequest;

pub async fn handle_request(req: Request<hyper::Body>) -> Result<Response<hyper::Body>> {
    // 解析请求，创建请求对象
    let data_request = DataRequest::new(&req)?;
    
    // 创建数据源管理器
    let manager = DataSourceManager::new();
    
    // 处理请求，根据缓存状态决定数据源策略
    match manager.process_request(req).await {
        Ok(response) => Ok(response),
        Err(e) => {
            // 统一错误处理
            let error_msg = format!("请求处理失败: {}", e);
            let response = Response::builder()
                .status(500)
                .body(hyper::Body::from(error_msg))
                .map_err(|e| ProxyError::Data(e.to_string()))?;
            Ok(response)
        }
    }
}
