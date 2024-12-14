
use proxy_server::config;

use proxy_server::server::run_server;
use proxy_server::utils::error::ProxyError;

#[tokio::main]
async fn main() -> Result<(), ProxyError> {
    println!("缓存目录: {:?}", config::CONFIG.cache_dir);
    run_server().await
}