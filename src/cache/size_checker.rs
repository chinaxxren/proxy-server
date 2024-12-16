use crate::utils::error::{Result, ProxyError};
use crate::config::CONFIG;
use crate::{log_error, log_info};
use hyper::{Client, Request, Body, Method};
use hyper::header::{CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use hyper_tls::HttpsConnector;
use serde_json::Value;
use tokio::fs;

pub struct SizeChecker {
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
}

impl SizeChecker {
    pub fn new() -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, Body>(https);
        Self { client }
    }

    pub async fn check_file_size(&self, url: &str) -> Result<u64> {
        // 1. 检查本地缓存状态文件
        if let Ok(size) = self.check_local_size(url).await {
            log_info!("SizeChecker", "从本地缓存获取文件大小: {}", size);
            return Ok(size);
        }

        // 2. 尝试 HEAD 请求
        if let Ok(size) = self.check_size_by_head(url).await {
            self.update_size_record(url, size).await?;
            return Ok(size);
        }

        // 3. 尝试 Range 请求
        if let Ok(size) = self.check_size_by_range(url).await {
            self.update_size_record(url, size).await?;
            return Ok(size);
        }

        // 4. 尝试普通 GET 请求
        if let Ok(size) = self.check_size_by_get(url).await {
            self.update_size_record(url, size).await?;
            return Ok(size);
        }

        Err(ProxyError::Request("无法获取文件大小".to_string()))
    }

    async fn check_local_size(&self, url: &str) -> Result<u64> {
        let state_path = CONFIG.get_cache_state(url);
        if let Ok(content) = fs::read_to_string(&state_path).await {
            if let Ok(json) = serde_json::from_str::<Value>(&content) {
                if let Some(total_size) = json.get("total_size").and_then(|v| v.as_u64()) {
                    return Ok(total_size);
                }
            }
        }
        Err(ProxyError::Cache("本地缓存中无文件大小信息".to_string()))
    }

    async fn check_size_by_head(&self, url: &str) -> Result<u64> {
        let req = Request::builder()
            .method(Method::HEAD)
            .uri(url)
            .body(Body::empty())?;

        let resp = self.client.request(req).await?;
        if let Some(len) = resp.headers().get(CONTENT_LENGTH) {
            if let Ok(size) = len.to_str()?.parse::<u64>() {
                log_info!("SizeChecker", "通过 HEAD 请求获取文件大小: {}", size);
                return Ok(size);
            }
        }
        Err(ProxyError::Request("HEAD 请求未返回文件大小".to_string()))
    }

    async fn check_size_by_range(&self, url: &str) -> Result<u64> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header(RANGE, "bytes=0-0")
            .body(Body::empty())?;

        let resp = self.client.request(req).await?;
        if let Some(range) = resp.headers().get(CONTENT_RANGE) {
            let range_str = range.to_str()?;
            if let Some(size_str) = range_str.split('/').last() {
                if let Ok(size) = size_str.parse::<u64>() {
                    log_info!("SizeChecker", "通过 Range 请求获取文件大小: {}", size);
                    return Ok(size);
                }
            }
        }
        Err(ProxyError::Request("Range 请求未返回文件大小".to_string()))
    }

    async fn check_size_by_get(&self, url: &str) -> Result<u64> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .body(Body::empty())?;

        let resp = self.client.request(req).await?;
        if let Some(len) = resp.headers().get(CONTENT_LENGTH) {
            if let Ok(size) = len.to_str()?.parse::<u64>() {
                log_info!("SizeChecker", "通过 GET 请求获取文件大小: {}", size);
                return Ok(size);
            }
        }
        Err(ProxyError::Request("GET 请求未返回文件大小".to_string()))
    }

    async fn update_size_record(&self, url: &str, size: u64) -> Result<()> {
        let state_path = CONFIG.get_cache_state(url);
        let mut json = if let Ok(content) = fs::read_to_string(&state_path).await {
            serde_json::from_str(&content)?
        } else {
            serde_json::json!({
                "ranges": [],
                "last_accessed": chrono::Utc::now().to_rfc3339(),
                "total_size": size,
                "cache_file": CONFIG.get_cache_file(url),
                "allocated_size": size
            })
        };

        // 只有当新大小大于已记录大小时才更新
        if let Some(current_size) = json.get("total_size").and_then(|v| v.as_u64()) {
            if size <= current_size {
                return Ok(());
            }
        }

        // 更新文件大小和已分配大小
        json["total_size"] = serde_json::json!(size);
        json["allocated_size"] = serde_json::json!(size);
        
        // 确保目录存在
        if let Some(parent) = std::path::Path::new(&state_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        // 写入文件
        fs::write(&state_path, serde_json::to_string_pretty(&json)?).await?;
        log_info!("SizeChecker", "更新文件大小记录: {} -> {}", url, size);
        
        Ok(())
    }
} 