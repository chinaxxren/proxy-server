use std::env;
use std::path::PathBuf;

pub struct Config {
    pub cache_dir: PathBuf,
}

impl Config {
    pub fn new() -> Self {
        let cache_dir = env::var("CACHE_DIR")
            .unwrap_or_else(|_| "cache".to_string())
            .into();

        // 确保缓存目录存在
        std::fs::create_dir_all(&cache_dir).expect("无法创建缓存目录");

        Self { cache_dir }
    }

    pub fn get_cache_path(&self, url: &str) -> PathBuf {
        self.cache_dir.join(url.replace("://", "_").replace("/", "_"))
    }
}

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = Config::new();
}