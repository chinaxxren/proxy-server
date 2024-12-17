use std::error::Error;
use std::fmt;
use std::io;
use tokio::sync::AcquireError;
use std::str::Utf8Error;

/// 全局结果集类型
pub type Result<T> = std::result::Result<T, ProxyError>;

#[derive(Debug, Clone)]
pub enum ProxyError {
    Cache(String),
    Network(String),
    InvalidRange(String),
    Range(String),
    Request(String),
    Storage(String),
    Parse(String),
    IO(String),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::Cache(msg) => write!(f, "Cache error: {}", msg),
            ProxyError::Network(msg) => write!(f, "Network error: {}", msg),
            ProxyError::InvalidRange(msg) => write!(f, "Invalid range error: {}", msg),
            ProxyError::Range(msg) => write!(f, "Range error: {}", msg),
            ProxyError::Request(msg) => write!(f, "Request error: {}", msg),
            ProxyError::Storage(msg) => write!(f, "Storage error: {}", msg),
            ProxyError::Parse(msg) => write!(f, "Parse error: {}", msg),
            ProxyError::IO(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl Error for ProxyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

impl From<hyper::Error> for ProxyError {
    fn from(err: hyper::Error) -> Self {
        ProxyError::Network(err.to_string())
    }
}

impl From<io::Error> for ProxyError {
    fn from(err: io::Error) -> Self {
        ProxyError::IO(err.to_string())
    }
}

impl From<std::num::ParseIntError> for ProxyError {
    fn from(err: std::num::ParseIntError) -> Self {
        ProxyError::Parse(err.to_string())
    }
}

impl From<serde_json::Error> for ProxyError {
    fn from(err: serde_json::Error) -> Self {
        ProxyError::Parse(err.to_string())
    }
}

impl From<hyper::http::Error> for ProxyError {
    fn from(err: hyper::http::Error) -> Self {
        ProxyError::Request(err.to_string())
    }
}

impl From<hyper::header::ToStrError> for ProxyError {
    fn from(err: hyper::header::ToStrError) -> Self {
        ProxyError::Request(err.to_string())
    }
}

impl From<Utf8Error> for ProxyError {
    fn from(err: Utf8Error) -> Self {
        ProxyError::Parse(err.to_string())
    }
}

impl From<AcquireError> for ProxyError {
    fn from(_: AcquireError) -> Self {
        ProxyError::Storage("无法获取信号量".to_string())
    }
}

