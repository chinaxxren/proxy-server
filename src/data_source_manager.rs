use std::sync::Arc;
use std::path::PathBuf;
use hyper::{Body, Response};
use bytes::Bytes;
use futures::StreamExt;
use futures::SinkExt;
use tokio::sync::broadcast;

use crate::data_request::DataRequest;
use crate::data_source::NetSource;
use crate::utils::error::Result;
use crate::storage::{StorageManager, StorageManagerConfig, DiskStorage, StorageConfig};
use crate::log_info;

pub struct DataSourceManager {
    storage_manager: Arc<StorageManager<DiskStorage>>,
}

impl DataSourceManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        // 创建存储配置
        let storage_config = StorageConfig {
            root_path: cache_dir.clone(),
            chunk_size: 8192,
        };
        
        // 创建存储管理器配置
        let manager_config = StorageManagerConfig::default();
        
        // 创建存储引擎和管理器
        let storage_engine = DiskStorage::new(storage_config);
        let storage_manager = Arc::new(StorageManager::new(storage_engine, manager_config));
        
        Self {
            storage_manager,
        }
    }
    
    pub async fn process_request(&self, req: &DataRequest) -> Result<Response<Body>> {
        let url = req.get_url();
        let range = req.get_range();
        
        // 检查缓存
        let key = url.to_string();
        let (start, end) = crate::utils::range::parse_range(&range)?;
        
        // 尝试从存储中读取
        if let Ok(stream) = self.storage_manager.read(&key, (start, end)).await {
            log_info!("Cache", "从缓存读取: {} {}-{}", url, start, end);
            return Ok(Response::new(Body::wrap_stream(stream)));
        }
        
        // 如果没有缓存，从网络获取
        log_info!("Cache", "从网络获取: {} {}-{}", url, start, end);
        let net_source = NetSource::new(url, &range);
        let (resp, _content_length) = net_source.download_stream().await?;
        
        // 异步写入存储
        let storage_manager = self.storage_manager.clone();
        let key = key.clone();
        let (parts, body) = resp.into_parts();
        
        // 创建可以共享的流
        let (tx, rx) = futures::channel::mpsc::channel::<Result<Bytes>>(32);
        let (broadcast_tx, _) = broadcast::channel::<Result<Bytes>>(32);
        let mut stream = futures::StreamExt::map(body, |result| result.map_err(|e| e.into()));
        
        let mut tx_clone = tx.clone();
        let broadcast_tx_clone = broadcast_tx.clone();
        tokio::spawn(async move {
            while let Some(chunk) = stream.next().await {
                let chunk_clone = chunk.map(|b| b.clone());
                if tx_clone.send(chunk_clone.clone()).await.is_err() {
                    break;
                }
                let _ = broadcast_tx_clone.send(chunk_clone);
            }
        });
        
        // 使用 rx 来写入存储
        let storage_manager = storage_manager.clone();
        let key = key.clone();
        tokio::spawn(async move {
            if let Err(e) = storage_manager.write(&key, rx, (start, end)).await {
                log_info!("Cache", "写入缓存失败: {} - {}", key, e);
            }
        });
        
        // 创建新的通道用于响应
        let mut broadcast_rx = broadcast_tx.subscribe();
        let stream = async_stream::stream! {
            while let Ok(chunk) = broadcast_rx.recv().await {
                yield chunk;
            }
        };
        
        // 使用响应通道来返回数据
        Ok(Response::from_parts(parts, Body::wrap_stream(stream)))
    }
}
