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
        if !fs::try_exists(&cache_path).await? {
            return Err(ProxyError::Cache("缓存文件不存在".to_string()));
        }

        // 解析请求范围
        let (start, end) = crate::utils::range::parse_range(range)?;

        // 检查缓存状态
        let cache_map = self.unit_pool.cache_map.read().await;
        let cache_path = CONFIG.get_cache_file(url);
        
        if let Some(data_unit) = cache_map.get(&cache_path) {
            // 对于混合源，我们只需要验证文件存在即可
            // 具体的范围处理会在 StreamProcessor 中进行
            drop(cache_map);
            return Ok(FileSource::new(url, range));
        } else {
            // 尝试加载缓存状态
            drop(cache_map);
            self.unit_pool.load_cache_state(url).await?;
            
            let cache_map = self.unit_pool.cache_map.read().await;
            if let Some(_) = cache_map.get(&cache_path) {
                return Ok(FileSource::new(url, range));
            }
        }

        Err(ProxyError::Cache("缓存记录不存在".to_string()))
    }

    pub async fn get_net_source(&self, url: &str, range: &str) -> Result<NetSource> {
        Ok(NetSource::new(url, range))
    }

    pub async fn get_cached_data(&self, url: &str, range: &str) -> Result<Vec<u8>> {
        let reader = DataReader::new(url, range, &self.unit_pool);
        let data = reader.read().await?;

        if data.is_empty() {
            return Err(ProxyError::Cache("缓存数据为空".to_string()));
        }
        Ok(data)
    }

    // 检查缓存状态
    pub async fn check_cache_status(&self, url: &str, range: &str) -> Result<bool> {
        // 检查是否完全缓存
        if self.unit_pool.is_fully_cached(url, range).await? {
            return Ok(true);
        }

        // 检查缓存文件是否存在
        let cache_file = CONFIG.get_cache_file(url);
        if !fs::try_exists(&cache_file).await? {
            return Ok(false);
        }

        // 检查文件大小
        let metadata = fs::metadata(&cache_file).await?;
        if metadata.len() == 0 {
            fs::remove_file(&cache_file).await?;
            return Ok(false);
        }

        // 检查部分缓存
        Ok(self.unit_pool.is_partially_cached(url, range).await?)
    }

    // 优化缓存
    pub async fn optimize_cache(&self, url: &str) -> Result<()> {
        // 获取缓存文件路径
        let cache_file = CONFIG.get_cache_file(url);
        if !fs::try_exists(&cache_file).await? {
            return Ok(());
        }

        // 获取文件大小
        let metadata = fs::metadata(&cache_file).await?;
        let file_size = metadata.len();

        // 更新缓存单元的总大小
        self.unit_pool.set_total_size(url, file_size).await?;

        // 优化缓存区间
        let mut cache_map = self.unit_pool.cache_map.write().await;
        let cache_path = CONFIG.get_cache_file(url);
        if let Some(data_unit) = cache_map.get_mut(&cache_path) {
            // 获取所有区间
            let ranges = data_unit.ranges.clone();
            
            // 清空现有区间
            data_unit.ranges.clear();
            
            // 重新添加并合并区间
            for (start, end) in ranges {
                data_unit.add_range(start, end);
            }
        }

        // 保存缓存状态
        drop(cache_map);
        self.unit_pool.save_cache_state(url).await?;
        
        Ok(())
    }
}

impl Default for DataStorage {
    fn default() -> Self {
        Self::new()
    }
}