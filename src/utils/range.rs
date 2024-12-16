use crate::utils::error::{Result, ProxyError};

pub fn parse_range(range: &str) -> Result<(u64, u64)> {
    // 处理空范围
    if range.is_empty() {
        return Ok((0, 0));
    }

    // 检查前缀
    if !range.starts_with("bytes=") {
        return Err(ProxyError::Range("Invalid range format".to_string()));
    }

    // 去掉 "bytes=" 前缀
    let range_str = &range[6..];
    
    // 处理 "0-" 格式
    if range_str == "0-" {
        return Ok((0, u64::MAX));
    }

    // 分割范围
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        return Err(ProxyError::Range("Invalid range format".to_string()));
    }

    // 解析开始位置
    let start = parts[0].parse::<u64>()
        .map_err(|_| ProxyError::Range("Invalid start position".to_string()))?;

    // 解析结束位置
    let end = if parts[1].is_empty() {
        u64::MAX  // 如果结束位置为空，使用最大值
    } else {
        parts[1].parse::<u64>()
            .map_err(|_| ProxyError::Range("Invalid end position".to_string()))?
    };

    // 验证范围
    if start > end && end != u64::MAX {
        return Err(ProxyError::Range("Invalid range: start > end".to_string()));
    }

    Ok((start, end))
}
