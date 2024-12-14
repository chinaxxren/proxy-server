use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use crate::request_handler::handle_request;
use crate::utils::error::{Result, ProxyError};
use std::net::SocketAddr;

pub async fn run_server() -> Result<()> {
    // 设置服务器地址和端口
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));

    // 创建服务
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, ProxyError>(service_fn(|req| async {
            // 处理请求，包括缓存检查和数据获取
            handle_request(req).await
        }))
    });

    // 创建并启动服务器
    let server = Server::bind(&addr).serve(make_svc);

    println!("代理服务器正在运行在 http://{}", addr);

    // 等待服务器运行
    server.await.map_err(|e| ProxyError::Http(e))
}
