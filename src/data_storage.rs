use crate::cache::unit_pool::UnitPool;
use crate::data_reader::DataReader;
use crate::utils::error::{ProxyError, Result};
use crate::data_source::{FileSource, NetSource};
use crate::config::CONFIG;
use crate::{log_error, log_info};
use tokio::fs;

pub struct DataStorage {
    unit_pool: UnitPool,
}

impl DataStorage {
    pub fn new() -> Self {
        Self {
            unit_pool: UnitPool::new(),
        }
    }

    pub async fn get_file_source(&self, url: &str, range: &str) -> Result<FileSource> {
        // 检查缓存文件是否存在
        let cache_path = CONFIG.get_cache_file(url);
        log_info!("Storage", "检查缓存文件: {}", cache_path);
        
        if !fs::try_exists(&cache_path).await? {
            log_error!("Storage", "缓存文件不存在: {}", cache_path);
            return Err(ProxyError::Cache("缓存文件不存在".to_string()));
        }

        // 检查缓存状态
        let cache_map = self.unit_pool.cache_map.read().await;
        let cache_path = CONFIG.get_cache_file(url);
        
        if let Some(data_unit) = cache_map.get(&cache_path) {
            log_info!("Storage", "找到缓存记录: {:?}", data_unit);
            drop(cache_map);
            return Ok(FileSource::new(url, range));
        } else {
            // 尝试加载缓存状态
            log_info!("Storage", "缓存记录不存在，尝试加载缓存状态");
            drop(cache_map);
            self.unit_pool.load_cache_state(url).await?;
            
            let cache_map = self.unit_pool.cache_map.read().await;
            if let Some(data_unit) = cache_map.get(&cache_path) {
                log_info!("Storage", "成功加载缓存状态: {:?}", data_unit);
                return Ok(FileSource::new(url, range));
            }
        }

        log_error!("Storage", "缓存记录不存在: {}", cache_path);
        Err(ProxyError::Cache("缓存记录不存在".to_string()))
    }

    pub async fn get_net_source(&self, url: &str, range: &str) -> Result<NetSource> {
        log_info!("Storage", "创建网络数据源: {} {}", url, range);
        Ok(NetSource::new(url, range))
    }

    pub async fn get_cached_data(&self, url: &str, range: &str) -> Result<Vec<u8>> {
        log_info!("Storage", "读取缓存数据: {} {}", url, range);
        let reader = DataReader::new(url, range, &self.unit_pool);
        let data = reader.read().await?;

        if data.is_empty() {
            log_error!("Storage", "缓存数据为空: {} {}", url, range);
            return Err(ProxyError::Cache("缓存数据为空".to_string()));
        }
        log_info!("Storage", "成功读取缓存数据: {} bytes", data.len());
        Ok(data)
    }

    pub async fn check_cache_status(&self, url: &str, range: &str) -> Result<bool> {
        log_info!("Storage", "检查缓存状态: {} {}", url, range);
        
        // 检查是否完全缓存
        if self.unit_pool.is_fully_cached(url, range).await? {
            log_info!("Storage", "完全缓存命中");
            return Ok(true);
        }

        // 检查缓存文件是否存在
        let cache_file = CONFIG.get_cache_file(url);
        if !fs::try_exists(&cache_file).await? {
            log_info!("Storage", "缓存文件不存在: {}", cache_file);
            return Ok(false);
        }

        // 检查文件大小
        let metadata = fs::metadata(&cache_file).await?;
        if metadata.len() == 0 {
            log_info!("Storage", "缓存文件大小为0，删除文件: {}", cache_file);
            fs::remove_file(&cache_file).await?;
            return Ok(false);
        }

        // 检查部分缓存
        let is_partial = self.unit_pool.is_partially_cached(url, range).await?;
        log_info!("Storage", "部分缓存检查结果: {}", is_partial);
        Ok(is_partial)
    }

    pub async fn optimize_cache(&self, url: &str) -> Result<()> {
        log_info!("Storage", "开始优化缓存: {}", url);
        
        // 获取缓存文件路径
        let cache_file = CONFIG.get_cache_file(url);
        if !fs::try_exists(&cache_file).await? {
            log_info!("Storage", "缓存文件不存在，跳过优化: {}", cache_file);
            return Ok(());
        }

        // 获取文件大小
        let metadata = fs::metadata(&cache_file).await?;
        let file_size = metadata.len();
        log_info!("Storage", "缓存文件大小: {} bytes", file_size);

        // 更新缓存单元的总大小
        self.unit_pool.set_total_size(url, file_size).await?;

        // 优化缓存区间
        let mut cache_map = self.unit_pool.cache_map.write().await;
        let cache_path = CONFIG.get_cache_file(url);
        if let Some(data_unit) = cache_map.get_mut(&cache_path) {
            log_info!("Storage", "开始合并缓存区间");
            // 获取所有区间
            let ranges = data_unit.ranges.clone();
            
            // 清空现有区间
            data_unit.ranges.clear();
            
            // 重新添加并合并区间
            for (start, end) in ranges {
                data_unit.add_range(start, end);
            }
            log_info!("Storage", "缓存区间合并完成: {:?}", data_unit.ranges);
        }

        // 保存缓存状态
        drop(cache_map);
        self.unit_pool.save_cache_state(url).await?;
        log_info!("Storage", "缓存优化完成");
        
        Ok(())
    }
}

impl Default for DataStorage {
    fn default() -> Self {
        Self::new()
    }
}