use std::path::PathBuf;
use url::Url;
use crate::utils::error::{Result, ProxyError};

pub struct Config {
    pub cache_dir: String,
}

impl Config {
    pub fn new(cache_dir: String) -> Self {
        Self {
            cache_dir,
        }
    }
    
    pub fn get_cache_state(&self, url: &str) -> Result<PathBuf> {
        let mut state_path = self.get_cache_file(url)?;
        state_path.set_extension("json");
        Ok(state_path)
    }
    
    pub fn get_cache_file(&self, url: &str) -> Result<PathBuf> {
        // 解析 URL
        let parsed_url = if url.starts_with("http://") || url.starts_with("https://") {
            Url::parse(url)
                .map_err(|e| ProxyError::Request(format!("无效的URL: {}", e)))?
        } else {
            // 如果是相对路径，尝试从请求头中获取原始 URL
            let base_url = Url::parse("http://localhost")
                .map_err(|e| ProxyError::Request(format!("无效的URL: {}", e)))?;
            base_url.join(url)
                .map_err(|e| ProxyError::Request(format!("无效的URL: {}", e)))?
        };
        
        // 创建缓存目录
        let mut cache_path = PathBuf::from(&self.cache_dir);
        
        // 使用 URL 的主机名作为子目录
        if let Some(host) = parsed_url.host_str() {
            cache_path.push(host);
        }
        
        // 使用 URL 的路径作为文件名
        let path = parsed_url.path();
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        
        for segment in segments.iter().take(segments.len() - 1) {
            cache_path.push(segment);
        }
        
        if let Some(last_segment) = segments.last() {
            cache_path.push(last_segment);
        } else {
            cache_path.push("index");
        }
        
        // 创建所需的目录
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ProxyError::Io(e))?;
        }
        
        Ok(cache_path)
    }
}

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = Config::new("cache".to_string());
}
