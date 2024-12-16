pub mod file_source;
pub mod net_source;

pub use file_source::FileSource;
pub use net_source::NetSource;

#[derive(Debug)]
pub enum DataSource {
    File(FileSource),
    Net(NetSource),
}
