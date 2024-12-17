#[macro_use]
extern crate lazy_static;

#[macro_export]
macro_rules! log_info {
    ($tag:expr, $($arg:tt)*) => {
        println!("[{} INFO {}] {}", 
            chrono::Local::now().format("%H:%M:%S"),
            $tag,
            format!($($arg)*)
        )
    };
}

pub mod data_request;
pub mod data_source;
pub mod data_source_manager;
pub mod storage;
pub mod utils;
pub mod handlers;
pub mod server;
pub mod hls;
pub mod request_handler;

pub use data_request::DataRequest;
pub use data_source_manager::DataSourceManager;



