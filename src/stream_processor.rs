use crate::cache::unit_pool::UnitPool;
use crate::config::CONFIG;
use crate::data_source::{FileSource, NetSource};
use crate::utils::error::Result;
use hyper::{Body, Response};
use std::sync::Arc;
use crate::log_info;
use url;

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
        let (req_start, req_end) = crate::utils::range::parse_range(&source.range)?;

        // 判断 source.path 是否是 URL
        if let Ok(parsed_url) = url::Url::parse(&source.path) {
            // 如果是 URL，则尝试从缓存中获取数据单元
            if let Some(data_unit) = self.unit_pool.get_data_unit(&source.path).await? {
                if data_unit.is_fully_cached(req_start, req_end) {
                    log_info!("Stream", "数据已完全缓存，直接读取缓存文件");
                    let stream = source.read_stream().await?;
                    return Ok(Response::new(Body::wrap_stream(stream)));
                }
            }

            // 如果不是 URL 或者缓存中没有数据单元，则更新缓存状态
            let stream = source.read_stream().await?;
            log_info!("Stream", "更新缓存状态: {} {}-{}", source.path, req_start, req_end);
            self.unit_pool.update_cache(&source.path, &source.range).await?;
            Ok(Response::new(Body::wrap_stream(stream)))
        } else {
            // 如果不是 URL，则假定它是本地文件路径，直接读取文件
            log_info!("Stream", "读取本地文件: {}", source.path);
            let stream = source.read_stream().await?;
            Ok(Response::new(Body::wrap_stream(stream)))
        }
    }
    pub async fn process_net_source(&self, source: NetSource) -> Result<Response<Body>> {
        let (resp, _content_length) = source.download_stream().await?;
        
        // 更新缓存状态
        if let Some((start, end)) = self.parse_content_range(resp.headers()) {
            let range = format!("bytes={}-{}", start, end);
            log_info!("Stream", "更新缓存状态: {} {}-{}", source.url, start, end);
            self.unit_pool.update_cache(&source.url, &range).await?;
        }
        
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
        
        // 检查是否有完整缓存
        if data_unit.is_fully_cached(start, end) {
            log_info!("Stream", "使用完整缓存: {}-{}", start, end);
            // 直接从 data_unit 读取数据
            let file_source = FileSource::new(&data_unit.cache_file, range);
            let stream = file_source.read_stream().await?;
            return Ok(Response::new(Body::wrap_stream(stream)));
        }
        
        // 获取已缓存的范围
        let cached_ranges = data_unit.get_cached_ranges(start, end);
        if !cached_ranges.is_empty() {
            log_info!("Stream", "使用混合源: 缓存范围 {:?}, 请求范围 {}-{}", cached_ranges, start, end);
            
            // 创建文件源和网络源
            let mut uncached_ranges = Vec::new();
            let mut current = start;
            
            for &(cache_start, cache_end) in &cached_ranges {
                if current < cache_start {
                    uncached_ranges.push((current, cache_start - 1));
                }
                current = cache_end + 1;
            }
            
            if current <= end {
                uncached_ranges.push((current, end));
            }
            
            log_info!("Stream", "未缓存范围: {:?}", uncached_ranges);
            
            // 只有当有未缓存的范围时，才从网络获取
            if !uncached_ranges.is_empty() {
                for (net_start, net_end) in uncached_ranges {
                    let net_range = format!("bytes={}-{}", net_start, net_end);
                    let net_source = NetSource::new(url, &net_range);
                    let _ = self.process_net_source(net_source).await?;
                }
                // 从网络获取数据后，重新获取数据单元，因为缓存范围已经更新
                if let Some(updated_data_unit) = self.unit_pool.get_data_unit(url).await? {
                    // 最后从缓存读取完整范围
                    // 使用 URL 创建 FileSource
                    let file_source = FileSource::new(url, range);
                    let response = self.process_file_source(file_source).await?;
                    return Ok(response);
                } else {
                    // 如果更新后的数据单元仍然不存在，返回错误
                    return Err(crate::utils::error::ProxyError::Cache("更新缓存后，数据单元仍然不存在".to_string()));
                }
            } else {
                // 如果没有未缓存的范围，直接从缓存读取
                let file_source = FileSource::new(&data_unit.cache_file, range);
                let stream = file_source.read_stream().await?;
                return Ok(Response::new(Body::wrap_stream(stream)));
            }
        } else {
            log_info!("Stream", "使用网络源: {}-{}", start, end);
            // 没有缓存，直接从网络获取
            let net_source = NetSource::new(url, range);
            let _resp = self.process_net_source(net_source).await?;

            // 重新获取 data_unit
            if let Some(updated_data_unit) = self.unit_pool.get_data_unit(url).await? {
                // 从缓存读取完整范围
                let file_source = FileSource::new(&updated_data_unit.cache_file, range);
                self.process_file_source(file_source).await
            } else {
                // 如果更新后的数据单元仍然不存在，返回错误
                return Err(crate::utils::error::ProxyError::Cache("更新缓存后，数据单元仍然不存在".to_string()));
            }
        }
    }
    
    fn parse_content_range(&self, headers: &hyper::HeaderMap) -> Option<(u64, u64)> {
        headers.get(hyper::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                let parts: Vec<&str> = s.split(' ').collect();
                if parts.len() == 2 && parts[0] == "bytes" {
                    let range_parts: Vec<&str> = parts[1].split('/').collect();
                    if range_parts.len() == 2 {
                        let range_nums: Vec<&str> = range_parts[0].split('-').collect();
                        if range_nums.len() == 2 {
                            let start = range_nums[0].parse::<u64>().ok()?;
                            let end = range_nums[1].parse::<u64>().ok()?;
                            return Some((start, end));
                        }
                    }
                }
                None
            })
    }
}
