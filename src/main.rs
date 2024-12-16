use proxy_server::config::Config;
use proxy_server::server::run_server_with_port;
use proxy_server::utils::error::ProxyError;
use proxy_server::log_info;
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

    // 初始化配置
    let _config = Config::with_cache_dir(cache_dir);
    log_info!("Main", "缓存目录: {}", cache_dir);
    
    // 启动服务器
    run_server_with_port(port).await
}