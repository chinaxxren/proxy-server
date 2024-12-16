use hyper::{Client, Request, Body};
use tokio;
use proxy_server::{log_info, server};
use hyper::body::to_bytes;

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

    // 创建 HTTP 客户端
    let client = Client::new();
    let video_url = "https://www.w3school.com.cn/i/movie.mp4";

    // 第一次请求：获取前 100KB 数据
    log_info!("Example", "第一步：请求前 100KB 数据");
    let req = Request::builder()
        .method("GET")
        .uri("http://127.0.0.1:8080")
        .header("Range", "bytes=0-102399")  // 0-100KB
        .header("X-Original-Url", video_url)
        .body(Body::empty())?;

    let resp = client.request(req).await?;
    log_info!("Example", "第一次响应状态: {}", resp.status());
    log_info!("Example", "第一次响应头: {:#?}", resp.headers());
    let body_bytes = to_bytes(resp.into_body()).await?;
    log_info!("Example", "第一次响应体长度: {}", body_bytes.len());

    // 等待确保缓存写入
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 第二次请求：请求 50KB-150KB 的数据（部分缓存，部分网络）
    log_info!("Example", "第二步：请求 50KB-150KB 数据（混合源）");
    let req = Request::builder()
        .method("GET")
        .uri("http://127.0.0.1:8080")
        .header("Range", "bytes=51200-153599")  // 50KB-150KB
        .header("X-Original-Url", video_url)
        .body(Body::empty())?;

    let resp = client.request(req).await?;
    log_info!("Example", "第二次响应状态: {}", resp.status());
    log_info!("Example", "第二次响应头: {:#?}", resp.headers());
    let body_bytes = to_bytes(resp.into_body()).await?;
    log_info!("Example", "第二次响应体长度: {}", body_bytes.len());

    // 第三次请求：再次请求相同范围，验证是否完全缓存
    log_info!("Example", "第三步：再次请求相同范围（验证缓存）");
    let req = Request::builder()
        .method("GET")
        .uri("http://127.0.0.1:8080")
        .header("Range", "bytes=51200-153599")  // 相同范围
        .header("X-Original-Url", video_url)
        .body(Body::empty())?;

    let resp = client.request(req).await?;
    log_info!("Example", "第三次响应状态: {}", resp.status());
    log_info!("Example", "第三次响应头: {:#?}", resp.headers());
    let body_bytes = to_bytes(resp.into_body()).await?;
    log_info!("Example", "第三次响应体长度: {}", body_bytes.len());

    Ok(())
} 