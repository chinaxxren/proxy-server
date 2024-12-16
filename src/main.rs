use proxy_server::server::ProxyServer;
use proxy_server::utils::error::ProxyError;
use std::env;

#[tokio::main]
async fn main() -> Result<(), ProxyError> {
    // 解析命令行参数
    let args: Vec<String> = env::args().collect();
    
    // 获取端口号，默认为 8080
    let port = if args.len() > 1 {
        args[1].parse().unwrap_or(8080)
    } else {
        8080
    };

    // 获取缓存目录，默认为 cache
    let cache_dir = if args.len() > 2 {
        &args[2]
    } else {
        "cache"
    };

    // 启动服务器
    let server = ProxyServer::new(port, cache_dir);
    let _ = server.start().await;
    
    Ok(())
}
