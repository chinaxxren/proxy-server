use hyper::{Client, Request, Body};
use tokio;
use proxy_server::{log_info, server};
use hyper::body::to_bytes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 启动代理服务器
    tokio::spawn(async {
        log_info!("Example", "启动代理服务器...");
        server::run_server().await.unwrap();
    });

    // 等待服务器启动
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // 创建 HTTP 客户端
    let client = Client::new();
    let video_url = "https://www.w3school.com.cn/i/movie.mp4";

    // 发送请求
    log_info!("Example", "发送单次请求");
    let req = Request::builder()
        .method("GET")
        .uri("http://127.0.0.1:8080")
        // .header("Range", "bytes=0-1023")
        .header("X-Original-Url", video_url)
        .body(Body::empty())?;

    let resp = client.request(req).await?;
    // 打印body 的长度
    log_info!("Example", "响应状态: {}", resp.status());

    log_info!("Example", "响应头: {:#?}", resp.headers());
    
    let body_bytes = to_bytes(resp.into_body()).await?;
    log_info!("Example", "响应体长度: {}", body_bytes.len());

    Ok(())
}
