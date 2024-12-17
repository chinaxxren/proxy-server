use std::error::Error;
use std::fmt;
use std::io;
use tokio::sync::AcquireError;
use std::str::Utf8Error;

/// 全局结果集类型
pub type Result<T> = std::result::Result<T, ProxyError>;

#[derive(Debug, Clone)]
pub enum ProxyError {
    Http(String),
    Io(String),
    Cache(String),
    DataParse(String),
    Network(String),
    File(String),
    Range(String),
    CacheMerge(String),
    Data(String),
    Request(String),
    Response(String),
    SerdeError(String),
    Parse(String),
    HttpHeader(String),
    Utf8(String),
    Json(String),
    Semaphore(String),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::Http(e) => write!(f, "HTTP error: {}", e),
            ProxyError::Io(e) => write!(f, "IO error: {}", e),
            ProxyError::Cache(s) => write!(f, "缓存错误: {}", s),
            ProxyError::DataParse(s) => write!(f, "数据解析错误: {}", s),
            ProxyError::Network(s) => write!(f, "网络错误: {}", s),
            ProxyError::File(s) => write!(f, "文件错误: {}", s),
            ProxyError::Range(s) => write!(f, "Range错误: {}", s),
            ProxyError::CacheMerge(s) => write!(f, "缓存合并错误: {}", s),
            ProxyError::Data(s) => write!(f, "数据错误: {}", s),
            ProxyError::Request(s) => write!(f, "请求错误: {}", s),
            ProxyError::Response(s) => write!(f, "响应错误: {}", s),
            ProxyError::SerdeError(s) => write!(f, "序列化错误: {}", s),
            ProxyError::Parse(s) => write!(f, "解析错误: {}", s),
            ProxyError::HttpHeader(s) => write!(f, "HTTP头错误: {}", s),
            ProxyError::Utf8(s) => write!(f, "UTF-8错误: {}", s),
            ProxyError::Json(s) => write!(f, "JSON错误: {}", s),
            ProxyError::Semaphore(s) => write!(f, "信号量错误: {}", s),
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
        ProxyError::Http(err.to_string())
    }
}

impl From<io::Error> for ProxyError {
    fn from(err: io::Error) -> Self {
        ProxyError::Io(err.to_string())
    }
}

impl From<std::num::ParseIntError> for ProxyError {
    fn from(err: std::num::ParseIntError) -> Self {
        ProxyError::DataParse(err.to_string())
    }
}

impl From<serde_json::Error> for ProxyError {
    fn from(err: serde_json::Error) -> Self {
        ProxyError::SerdeError(err.to_string())
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
        ProxyError::Utf8(err.to_string())
    }
}

impl From<AcquireError> for ProxyError {
    fn from(_: AcquireError) -> Self {
        ProxyError::Semaphore("无法获取信号量".to_string())
    }
}

