use proxy_server::config;
use proxy_server::server::run_server;
use proxy_server::utils::error::ProxyError;
use proxy_server::log_info;

#[tokio::main]
async fn main() -> Result<(), ProxyError> {
    log_info!("Main", "缓存目录: {:?}", config::CONFIG.cache_dir);
    run_server().await
}