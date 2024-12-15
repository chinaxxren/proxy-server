use crate::utils::error::{ProxyError, Result};

/// 解析 HTTP Range 头部
/// 
/// # Arguments
/// * `range` - Range 头部值，格式如 "bytes=0-1023"
/// 
/// # Returns
/// * `Ok((start, end))` - 解析成功，返回起始和结束位置
/// * `Err(ProxyError)` - 解析失败
pub fn parse_range(range: &str) -> Result<(u64, u64)> {
    if range.starts_with("bytes=") {
        let bytes = &range[6..];
        let parts: Vec<&str> = bytes.split('-').collect();

        let start = parts[0].parse::<u64>()
                .map_err(|e| ProxyError::DataParse(e.to_string()))?;
        if parts.len() == 2 {
            let end = parts[1].parse::<u64>()
                .map_err(|e| ProxyError::DataParse(e.to_string()))?;
            if start <= end {
                return Ok((start, end));
            }
        }

        return Ok((start, u64::MAX));
    }

    Err(ProxyError::Range("Invalid Range header format".to_string()))
}