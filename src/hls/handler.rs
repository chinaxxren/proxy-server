use crate::utils::error::{ProxyError, Result};
use crate::data_request::DataRequest;
use crate::data_source_manager::DataSourceManager;
use crate::log_info;
use super::{HlsHandler, HlsManager};
use hyper::Client;
use hyper_tls::HttpsConnector;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;
use urlencoding;

pub struct DefaultHlsHandler {
    manager: Arc<HlsManager>,
    source_manager: Arc<DataSourceManager>,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
}

impl DefaultHlsHandler {
    pub fn new(cache_dir: PathBuf, source_manager: Arc<DataSourceManager>) -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);
        
        Self {
            manager: Arc::new(HlsManager::new(cache_dir)),
            source_manager,
            client,
        }
    }

    fn get_base_url(&self, url: &str) -> Result<String> {
        let parsed = Url::parse(url)
            .map_err(|e| ProxyError::Parse(format!("无法解析URL: {}", e)))?;
        
        let mut base = parsed.clone();
        if let Some(segments) = base.path_segments() {
            let segments: Vec<_> = segments.collect();
            if !segments.is_empty() {
                base.path_segments_mut()
                    .map_err(|_| ProxyError::Parse("无法修改URL路径".to_string()))?
                    .pop();
            }
        }
        
        Ok(base.to_string())
    }

    async fn download_m3u8(&self, url: &str) -> Result<String> {
        log_info!("HLS", "下载 m3u8 文件: {}", url);
        
        let req = DataRequest::new_request_with_range(url, "bytes=0-");
        let resp = self.client.request(req).await
            .map_err(|e| ProxyError::Network(format!("请求失败: {}", e)))?;
        
        if !resp.status().is_success() {
            return Err(ProxyError::Network(format!("请求失败: {}", resp.status())));
        }
        
        let body = hyper::body::to_bytes(resp.into_body()).await
            .map_err(|e| ProxyError::Network(format!("读取响应失败: {}", e)))?;
        
        String::from_utf8(body.to_vec())
            .map_err(|e| ProxyError::Parse(format!("解析响应内容失败: {}", e)))
    }
}

#[async_trait::async_trait]
impl HlsHandler for DefaultHlsHandler {
    async fn handle_m3u8(&self, url: &str) -> Result<String> {
        log_info!("HLS", "处理 m3u8 请求: {}", url);
        
        // 移除可能存在的 /proxy/ 前缀
        let clean_url = if let Some(proxy_path) = url.find("/proxy/") {
            let url_part = &url[proxy_path + 7..];
            // 处理可能存在的多重 /proxy/ 前缀
            let mut clean = url_part.to_string();
            while let Some(idx) = clean.find("/proxy/") {
                clean = clean[idx + 7..].to_string();
            }
            // 解码 URL
            urlencoding::decode(&clean)
                .map_err(|e| ProxyError::Request(format!("URL 解码失败: {}", e)))?
                .into_owned()
        } else {
            url.to_string()
        };
        
        // 下载 m3u8 内容
        let content = self.download_m3u8(&clean_url).await?;
        
        // 处理 m3u8 文件
        let _info = self.manager.process_m3u8(&clean_url, &content).await?;
        
        // 获取基础 URL
        let base_url = self.get_base_url(&clean_url)?;
        
        // 重写 m3u8 内容
        let rewritten = self.manager.rewrite_m3u8(
            &content,
            &base_url,
            "/proxy"
        );
        
        Ok(rewritten)
    }
    
    async fn handle_segment(&self, url: &str, range: Option<String>) -> Result<Vec<u8>> {
        log_info!("HLS", "处理分片请求: {} range={:?}", url, range);
        
        // 创建数据请求
        let req = DataRequest::new_request_with_range(
            url,
            &range.unwrap_or_else(|| "bytes=0-".to_string())
        );
        
        // 使用数据源管理器处理请求
        let resp = self.source_manager.process_request(&DataRequest::new(&req)?).await?;
        
        // 读取响应体
        let body = hyper::body::to_bytes(resp.into_body()).await
            .map_err(|e| ProxyError::Network(format!("读取响应失败: {}", e)))?;
        
        Ok(body.to_vec())
    }
} 