use crate::cache::unit_pool::UnitPool;
use crate::utils::error::Result;
use crate::utils::parse_range;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;
use crate::config::CONFIG;

pub struct DataReader<'a> {
    url: &'a str,
    range: &'a str,
    _unit_pool: &'a UnitPool,
}

impl<'a> DataReader<'a> {
    pub fn new(url: &'a str, range: &'a str, unit_pool: &'a UnitPool) -> Self {
        Self { url, range, _unit_pool: unit_pool }
    }

    pub async fn read(&self) -> Result<Vec<u8>> {
        let file_path = CONFIG.get_cache_path(self.url);
        let mut file = File::open(&file_path).await?;
        
        let (start, end) = parse_range(self.range)?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut buffer = vec![0; (end - start + 1) as usize];
        file.read_exact(&mut buffer).await?;
        
        Ok(buffer)
    }
}
