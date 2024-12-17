#[macro_use]
pub mod macros;

pub mod config;
pub mod data_request;
pub mod data_source;
pub mod hls;
pub mod server;
pub mod utils;
pub mod request_handler;
pub mod data_source_manager;
pub mod storage;

pub use config::{Config, CONFIG};
pub use data_request::{DataRequest, RequestType};
pub use data_source::{file_source::FileSource, net_source::NetSource};
pub use hls::DefaultHlsHandler;
pub use server::{run_server, ProxyServer};
pub use utils::error::{Result, ProxyError};
pub use request_handler::RequestHandler;
pub use data_source_manager::DataSourceManager;
pub use storage::{StorageEngine, StorageManager, StorageManagerConfig, DiskStorage, StorageConfig};



