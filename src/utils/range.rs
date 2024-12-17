use crate::utils::error::{Result, ProxyError};

pub fn parse_range(range: &str) -> Result<(u64, u64)> {
    // 检查前缀
    if !range.starts_with("bytes=") {
        return Err(ProxyError::Request("Invalid range format".to_string()));
    }

    // 移除前缀
    let range = &range[6..];

    // 分割范围
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return Err(ProxyError::Request("Invalid range format".to_string()));
    }

    // 解析开始位置
    let start = parts[0]
        .parse::<u64>()
        .map_err(|_| ProxyError::Request("Invalid start position".to_string()))?;

    // 解析结束位置
    let end = if parts[1].is_empty() {
        u64::MAX
    } else {
        parts[1]
            .parse::<u64>()
            .map_err(|_| ProxyError::Request("Invalid end position".to_string()))?
    };

    // 验证范围
    if start > end {
        return Err(ProxyError::Request("Invalid range: start > end".to_string()));
    }

    Ok((start, end))
}
