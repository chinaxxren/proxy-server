use crate::cache::unit_pool::UnitPool;
use crate::data_reader::DataReader;
use crate::utils::error::{ProxyError, Result};
use crate::data_source::{FileSource, NetSource};
use tokio::io::AsyncWriteExt;
use tokio::fs;
use crate::config::CONFIG;
use tokio::io::AsyncSeekExt;

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
        let file_path = CONFIG.get_cache_path(url);
        if !file_path.exists() {
            return Err(ProxyError::Cache("缓存文件不存在".to_string()));
        }
        Ok(FileSource::new(url, range))
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

    // 写入缓存数据
    pub async fn _write_cache(&self, url: &str, range: &str, data: &[u8]) -> Result<()> {
        let file_path = CONFIG.get_cache_path(url);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&file_path)
            .await?;

        // 解析 Range 并写入对应位置
        let (start, _) = crate::utils::parse_range(range)?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        file.write_all(data).await?;

        // 更新缓存状态
        self.unit_pool.update_cache(url, range).await?;
        Ok(())
    }

    // 检查缓存状态
    pub async fn _check_cache_status(&self, url: &str, range: &str) -> Result<bool> {
        if self.unit_pool.is_fully_cached(url, range).await? {
            return Ok(true);
        }

        let file_path = CONFIG.get_cache_path(url);
        if !file_path.exists() {
            return Ok(false);
        }

        let metadata = fs::metadata(&file_path).await?;
        if metadata.len() == 0 {
            fs::remove_file(&file_path).await?;
            return Ok(false);
        }

        Ok(self.unit_pool.is_partially_cached(url, range).await?)
    }

    // 合并缓存文件
    pub async fn _merge_cache_files(&self, url: &str) -> Result<()> {
        let file_path = CONFIG.get_cache_path(url);
        if !file_path.exists() {
            return Ok(());
        }

        // 获取所有缓存区间
        let ranges = self.unit_pool.get_cached_ranges(url).await?;
        if ranges.is_empty() {
            return Ok(());
        }

        // 合并相邻区间
        self.unit_pool.merge_range(url, &ranges).await?;
        
        // 获取更新后的区间
        let merged_ranges = self.unit_pool.get_cached_ranges(url).await?;
        
        // 更新缓存状态
        self.unit_pool.update_cached_ranges(url, merged_ranges).await?;
        Ok(())
    }
}