use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use url::Url;

pub struct Config {
    pub cache_dir: String,
}

impl Config {
    pub fn new() -> Self {
        Self::with_cache_dir("cache")
    }

    pub fn with_cache_dir(cache_dir: &str) -> Self {
        // 确保缓存目录存在
        std::fs::create_dir_all(cache_dir).expect("无法创建缓存目录");
        Self { cache_dir: cache_dir.to_string() }
    }

    pub fn get_cache_state(&self, url: &str) -> String {
        let url_parts = Url::parse(url).expect("无效的URL");
        let host = url_parts.host_str().unwrap_or_default();
        let path = url_parts.path();
        let hash_path = format!("{}{}", host, path);

        let mut hasher = DefaultHasher::new();
        hash_path.hash(&mut hasher);
        let hash = hasher.finish();

        format!("{}/{:x}.json", self.cache_dir, hash)
    }

    pub fn get_cache_file(&self, url: &str) -> String {
        let url_parts = Url::parse(url).expect("无效的URL");
        let host = url_parts.host_str().unwrap_or_default();
        let path = url_parts.path();
        let hash_path = format!("{}{}", host, path);

        let mut hasher = DefaultHasher::new();
        hash_path.hash(&mut hasher);
        let hash = hasher.finish();

        format!("{}/{:x}.cache", self.cache_dir, hash)
    }

    pub fn get_cache_root(&self) -> String {
        return self.cache_dir.clone();
    }
}

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = Config::new();
}
