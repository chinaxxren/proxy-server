use hyper::{Body, Client, Request};
use proxy_server::{log_info, server};
use std::env;
use tokio::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 解析命令行参数
    let args: Vec<String> = env::args().collect();
    let target_url = if args.len() > 1 {
        &args[1]
    } else {
        "https://media.w3.org/2010/05/sintel/trailer.mp4"
    };

    // 启动代理服务器
    tokio::spawn(async {
        log_info!("Server", "启动代理服务器...");
        if let Err(e) = server::run_server().await {
            log_info!("Server", "代理服务器错误: {}", e);
        }
    });

    // 等待服务器启动
    tokio::time::sleep(Duration::from_secs(1)).await;

    let proxy_host = "127.0.0.1:8080";

    // 构建完整的代理URL
    let proxy_url = format!("http://{}/proxy/{}", proxy_host, target_url);

    // 创建 HTTP 客户端
    let client = Client::new();

    // 构建请求
    log_info!("Client", "代理URL: {}", proxy_url);
    log_info!("Client", "目标URL: {}", target_url);

    let req = Request::builder()
        .method("GET")
        .uri(&proxy_url)
        .body(Body::empty())?;

    // 发送请求
    log_info!("Client", "发送请求...");
    let resp = client.request(req).await?;

    // 输出响应信息
    log_info!("Client", "响应状态: {}", resp.status());
    log_info!("Client", "响应头: {:#?}", resp.headers());

    Ok(())
}
