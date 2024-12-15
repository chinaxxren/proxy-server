use crate::utils::error::{ProxyError, Result};

/// 解析 HTTP Range 头部
///
/// # Arguments
/// * `range` - Range 头部值，格式如 "bytes=0-1023" 或 "bytes=1024-"
///
/// # Returns
/// * `Ok((start, end))` - 解析成功，返回起始和结束位置
/// * `Err(ProxyError)` - 解析失败
pub fn parse_range(range: &str) -> Result<(u64, u64)> {
    if !range.starts_with("bytes=") {
        return Err(ProxyError::Range("Invalid Range header format".to_string()));
    }

    let bytes = &range[6..];
    
    // 处理 "bytes=数字-" 格式
    if bytes.ends_with('-') {
        return Err(ProxyError::Range("Invalid start position".to_string()));
    }
    
    // 处理 "bytes=数字-数字" 格式
    let parts: Vec<&str> = bytes.split('-').collect();
    if parts.len() == 2 {
        if let (Ok(start), Ok(end)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
            if start <= end {
                return Ok((start, end));
            }
            return Err(ProxyError::Range("Start position greater than end".to_string()));
        }
    }

    Err(ProxyError::Range("Invalid Range header format".to_string()))
}
