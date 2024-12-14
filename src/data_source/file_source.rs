use crate::config::CONFIG;
use crate::utils::parse_range;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;
use crate::utils::error::Result;

pub struct FileSource {
    path: String,
    range: String,
}

impl FileSource {
    pub fn new(url: &str, range: &str) -> Self {
        let path = CONFIG.get_cache_path(url).to_string_lossy().into_owned();
        Self { path, range: range.to_string() }
    }

    pub async fn read_data(&self) -> Result<Vec<u8>> {
        let mut file = tokio::fs::File::open(&self.path).await?;
        let (start, end) = parse_range(&self.range)?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut buffer = vec![0; (end - start + 1) as usize];
        file.read_exact(&mut buffer).await?;
        Ok(buffer)
    }
}
