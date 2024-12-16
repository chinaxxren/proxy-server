use hyper::{Client, Request, Body};
use hyper_tls::HttpsConnector;
use tokio;
use proxy_server::{log_info, server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 启动代理服务器
    tokio::spawn(async {
        log_info!("Example", "启动代理服务器...");
        let server = server::ProxyServer::new(8080, "./cache");
        server.start().await;
    });

    // 等待服务器启动
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    // 测试 HTTPS URL
    let https_url = "https://media.w3.org/2010/05/sintel/trailer.mp4";

    log_info!("Example", "发送 HTTPS 请求: {}", https_url);
    let req = Request::builder()
        .method("GET")
        .uri("http://127.0.0.1:8080")
        .header("X-Original-Url", https_url)
        .body(Body::empty())?;

    let resp = client.request(req).await?;
    log_info!("Example", "响应状态: {}", resp.status());
    log_info!("Example", "响应头: {:#?}", resp.headers());

    Ok(())
}
