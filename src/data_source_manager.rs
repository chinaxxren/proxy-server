use crate::data_request::DataRequest;
use crate::data_source::{file_source::FileSource, net_source::NetSource};
use crate::stream_processor::StreamProcessor;
use crate::utils::error::Result;
use crate::UnitPool;
use hyper::{Body, Response};
use std::path::PathBuf;
use std::sync::Arc;

pub struct DataSourceManager {
    unit_pool: Arc<UnitPool>,
    stream_processor: Arc<StreamProcessor>,
}

impl DataSourceManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        let unit_pool = Arc::new(UnitPool::new(cache_dir));
        let stream_processor = Arc::new(StreamProcessor::new(unit_pool.clone()));
        
        Self {
            unit_pool,
            stream_processor,
        }
    }
    
    pub async fn process_request(&self, req: &DataRequest) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();
        
        // 检查缓存
        if let Some(data_unit) = self.unit_pool.get_data_unit(url).await? {
            // 如果缓存完整，直接从缓存读取
            if data_unit.is_complete() {
                let file_source = FileSource::new(&data_unit.cache_file, range);
                return self.stream_processor.process_file_source(file_source).await;
            }
        }
        
        // 如果没有完整缓存，创建网络源
        let net_source = NetSource::new(url, range);
        self.stream_processor.process_net_source(net_source).await
    }
}
