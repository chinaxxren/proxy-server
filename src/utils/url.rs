use url::Url;

use crate::utils::error::{ProxyError, Result};

pub struct UrlUtils;

impl UrlUtils {
    /// 清理代理 URL 前缀
    /// 
    /// # Examples
    /// ```
    /// use proxy_server::utils::url::UrlUtils;
    /// 
    /// let url = "/proxy/http://example.com/video.mp4";
    /// let clean = UrlUtils::clean_proxy_url(url).unwrap();
    /// assert_eq!(clean, "http://example.com/video.mp4");
    /// ```
    pub fn clean_proxy_url(url: &str) -> Result<String> {
        if let Some(proxy_path) = url.find("/proxy/") {
            let url_part = &url[proxy_path + 7..];
            let mut clean = url_part.to_string();
            
            // 处理多重 /proxy/ 前缀
            while let Some(idx) = clean.find("/proxy/") {
                clean = clean[idx + 7..].to_string();
            }
            
            urlencoding::decode(&clean)
                .map(|s| s.into_owned())
                .map_err(|e| ProxyError::Request(format!("URL 解码失败: {}", e)))
        } else {
            Ok(url.to_string())
        }
    }

    /// 获取 URL 的基础路径
    /// 
    /// # Examples
    /// ```
    /// use proxy_server::utils::url::UrlUtils;
    /// 
    /// let url = "http://example.com/path/file.m3u8";
    /// let base = UrlUtils::get_base_url(url).unwrap();
    /// assert_eq!(base, "http://example.com/path/");
    /// ```
    pub fn get_base_url(url: &str) -> Result<String> {
        let parsed = Url::parse(url)
            .map_err(|e| ProxyError::Parse(format!("无法解析URL: {}", e)))?;
        
        let mut base = parsed.clone();
        let segments: Vec<_> = base.path_segments()
            .map(|s| s.collect())
            .unwrap_or_default();
        let mut segments_mut = base.path_segments_mut()
            .map_err(|_| ProxyError::Parse("无法修改URL路径".to_string()))?;
        segments_mut.clear();
        
        if !segments.is_empty() {
            segments_mut.extend(&segments[..segments.len() - 1]);
        }
        
        Ok(base.to_string())
    }

    /// 判断是否是完整的 URL
    pub fn is_absolute_url(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_proxy_url() {
        let cases = vec![
            ("/proxy/http://example.com", "http://example.com"),
            ("/proxy/proxy/http://example.com", "http://example.com"),
            ("http://example.com", "http://example.com"),
            ("/proxy/https://example.com/video.mp4", "https://example.com/video.mp4"),
            ("/proxy/proxy/proxy/http://test.com", "http://test.com"),
        ];

        for (input, expected) in cases {
            assert_eq!(UrlUtils::clean_proxy_url(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_get_base_url() {
        let cases = vec![
            ("http://example.com/video/file.m3u8", "http://example.com/video/"),
            ("http://example.com/file.m3u8", "http://example.com/"),
            ("http://example.com/", "http://example.com/"),
            ("https://test.com/a/b/c.m3u8", "https://test.com/a/b/"),
            ("http://example.com", "http://example.com/"),
        ];

        for (input, expected) in cases {
            assert_eq!(UrlUtils::get_base_url(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_is_absolute_url() {
        assert!(UrlUtils::is_absolute_url("http://example.com"));
        assert!(UrlUtils::is_absolute_url("https://example.com"));
        assert!(!UrlUtils::is_absolute_url("/path/to/file"));
        assert!(!UrlUtils::is_absolute_url("relative/path"));
    }
} 