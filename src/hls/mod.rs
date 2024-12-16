mod handler;

pub use handler::DefaultHlsHandler;

use std::path::PathBuf;
use async_trait::async_trait;
use m3u8_rs::Playlist;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use crate::utils::error::Result;
use crate::log_info;

/// HLS 分片信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// 分片 URL
    pub url: String,
    /// 分片时长（秒）
    pub duration: f32,
    /// 分片序号
    pub sequence: u64,
    /// 分片大小（字节）
    pub size: Option<u64>,
    /// 是否已缓存
    pub cached: bool,
}

/// HLS 播放列表信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistInfo {
    /// 原始 URL
    pub url: String,
    /// 目标持续时间
    pub target_duration: f32,
    /// 媒体序列号
    pub media_sequence: u64,
    /// 是否是直播流
    pub is_endlist: bool,
    /// 分片列表
    pub segments: Vec<Segment>,
    /// 变体流信息（仅用于主播放列表）
    pub variants: Vec<VariantStream>,
    /// 最后更新时间
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// 变体流信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantStream {
    /// 播放列表 URL
    pub url: String,
    /// 带宽（比特/秒）
    pub bandwidth: u64,
    /// 分辨率（可选）
    pub resolution: Option<String>,
}

/// HLS 缓存管理器
pub struct HlsManager {
    /// 缓存根目录
    cache_dir: PathBuf,
    /// 播放列表缓存
    playlists: Arc<RwLock<HashMap<String, PlaylistInfo>>>,
}

impl HlsManager {
    /// 创建新的 HLS 管理器实例
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            playlists: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 处理 m3u8 文件
    pub async fn process_m3u8(&self, url: &str, content: &str) -> Result<PlaylistInfo> {
        log_info!("HLS", "开始处理 m3u8 文件: {}", url);
        
        // 解析 m3u8 内容
        let playlist = m3u8_rs::parse_playlist(content.as_bytes())
            .map_err(|e| crate::utils::error::ProxyError::Parse(e.to_string()))?
            .1;  // 获取解析结果的第二个元素

        match playlist {
            m3u8_rs::Playlist::MasterPlaylist(master) => {
                log_info!("HLS", "处理主播放列表，包含 {} 个变体流", master.variants.len());
                
                // 处理主播放列表
                let variants = master
                    .variants
                    .iter()
                    .map(|v| VariantStream {
                        url: v.uri.clone(),
                        bandwidth: v.bandwidth,
                        resolution: v.resolution.as_ref().map(|r| format!("{}x{}", r.width, r.height)),
                    })
                    .collect();

                let info = PlaylistInfo {
                    url: url.to_string(),
                    target_duration: 0.0,
                    media_sequence: 0,
                    is_endlist: false,
                    segments: vec![],
                    variants,
                    last_updated: chrono::Utc::now(),
                };

                // 缓存播放列表信息
                self.playlists.write().await.insert(url.to_string(), info.clone());
                Ok(info)
            }
            m3u8_rs::Playlist::MediaPlaylist(media) => {
                log_info!("HLS", "处理媒体播放列表，包含 {} 个分片", media.segments.len());
                
                // 处理媒体播放列表
                let segments = media
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(i, s)| Segment {
                        url: s.uri.clone(),
                        duration: s.duration,
                        sequence: media.media_sequence + i as u64,
                        size: None,
                        cached: false,
                    })
                    .collect();

                let info = PlaylistInfo {
                    url: url.to_string(),
                    target_duration: media.target_duration,
                    media_sequence: media.media_sequence,
                    is_endlist: media.end_list,
                    segments,
                    variants: vec![],
                    last_updated: chrono::Utc::now(),
                };

                // 缓存播放列表信息
                self.playlists.write().await.insert(url.to_string(), info.clone());
                Ok(info)
            }
        }
    }

    /// 重写 m3u8 内容，将 URL 替换为代理 URL
    pub fn rewrite_m3u8(&self, content: &str, base_url: &str, proxy_prefix: &str) -> String {
        log_info!("HLS", "重写 m3u8 内容，base_url: {}", base_url);
        
        let mut result = String::new();
        for line in content.lines() {
            if line.starts_with('#') {
                // 处理带 URL 的标签
                if line.starts_with("#EXT-X-STREAM-INF:") {
                    result.push_str(line);
                    result.push('\n');
                    continue;
                }
                result.push_str(line);
                result.push('\n');
            } else if !line.is_empty() {
                // 处理 URL 行
                let url = if line.starts_with("http://") || line.starts_with("https://") {
                    line.to_string()
                } else if line.starts_with("/proxy/") {
                    // 如果已经是代理 URL，去掉前缀重新处理
                    let clean_url = &line[7..];
                    if clean_url.starts_with("http://") || clean_url.starts_with("https://") {
                        clean_url.to_string()
                    } else {
                        format!("{}/{}", base_url.trim_end_matches('/'), clean_url.trim_start_matches('/'))
                    }
                } else {
                    // 相对路径，需要拼接基础 URL
                    let base = base_url.trim_end_matches('/');
                    format!("{}/{}", base, line.trim_start_matches('/'))
                };

                // 添加代理前缀
                result.push_str(&format!("{}/{}\n", 
                    proxy_prefix.trim_end_matches('/'), 
                    urlencoding::encode(&url)
                ));
            }
        }
        result
    }

    /// 获取播放列表信息
    pub async fn get_playlist(&self, url: &str) -> Option<PlaylistInfo> {
        self.playlists.read().await.get(url).cloned()
    }

    /// 更新分片缓存状态
    pub async fn update_segment_cache(&self, url: &str, sequence: u64, size: u64) -> Result<()> {
        log_info!("HLS", "更新分片缓存状态: {} sequence={}", url, sequence);
        
        if let Some(playlist) = self.playlists.write().await.get_mut(url) {
            if let Some(segment) = playlist.segments.iter_mut().find(|s| s.sequence == sequence) {
                segment.size = Some(size);
                segment.cached = true;
            }
        }
        Ok(())
    }

    /// 获取分片的缓存路径
    pub fn get_segment_cache_path(&self, url: &str, sequence: u64) -> PathBuf {
        let hash = format!("{:x}", md5::compute(url));
        self.cache_dir.join(format!("{}_seg_{}.ts", hash, sequence))
    }
}

#[async_trait]
pub trait HlsHandler {
    /// 处理 m3u8 请求
    async fn handle_m3u8(&self, url: &str) -> Result<String>;
    
    /// 处理分片请求
    async fn handle_segment(&self, url: &str, range: Option<String>) -> Result<Vec<u8>>;
} 