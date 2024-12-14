use std::error::Error;
use std::fmt;
use std::io;

/// 全局结果集类型
pub type Result<T> = std::result::Result<T, ProxyError>;

#[derive(Debug)]
pub enum ProxyError {
    Http(hyper::Error),
    Io(io::Error),
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
        }
    }
}

impl Error for ProxyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ProxyError::Http(e) => Some(e),
            ProxyError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<hyper::Error> for ProxyError {
    fn from(err: hyper::Error) -> Self {
        ProxyError::Http(err)
    }
}

impl From<io::Error> for ProxyError {
    fn from(err: io::Error) -> Self {
        ProxyError::Io(err)
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

