use crate::cache::unit_pool::UnitPool;
use crate::config::CONFIG;
use crate::data_source::{FileSource, NetSource};
use crate::utils::error::Result;
use hyper::{Body, Response};
use std::sync::Arc;

pub struct StreamProcessor {
    unit_pool: Arc<UnitPool>,
}

impl StreamProcessor {
    pub fn new(unit_pool: Arc<UnitPool>) -> Self {
        Self {
            unit_pool,
        }
    }
    
    pub async fn process_file_source(&self, source: FileSource) -> Result<Response<Body>> {
        let stream = source.read_stream().await?;
        Ok(Response::new(Body::wrap_stream(stream)))
    }
    
    pub async fn process_net_source(&self, source: NetSource) -> Result<Response<Body>> {
        let (resp, _) = source.download_stream().await?;
        Ok(resp)
    }
    
    pub async fn process_mixed_source(&self, url: &str, range: &str) -> Result<Response<Body>> {
        let (start, end) = crate::utils::range::parse_range(range)?;
        let cache_path = CONFIG.get_cache_file(url)?;
        let cache_path_str = cache_path.to_string_lossy().into_owned();
        
        // 获取缓存状态
        let data_unit = if let Some(unit) = self.unit_pool.get_data_unit(url).await? {
            unit
        } else {
            UnitPool::new_data_unit(&cache_path_str)
        };
        
        // 检查是否需要扩展文件大小
        if data_unit.needs_allocation(end + 1) {
            self.unit_pool.ensure_file_size(&data_unit.cache_file, end + 1).await?;
        }
        
        // 更新缓存范围
        self.unit_pool.update_cache(url, range).await?;
        
        // 创建响应
        let response = Response::builder()
            .header("Content-Type", "application/octet-stream")
            .header("Accept-Ranges", "bytes")
            .header("Content-Range", format!("bytes {}-{}/{}", start, end, end + 1))
            .body(Body::empty())?;
        
        Ok(response)
    }
}
