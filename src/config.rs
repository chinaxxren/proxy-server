use std::path::{PathBuf, Path};
use crate::utils::error::Result;

pub struct Config {
    pub cache_dir: String,
}

impl Config {
    pub fn new(cache_dir: String) -> Self {
        Self {
            cache_dir,
        }
    }
    
    pub fn get_cache_state(&self, url: &str) -> Result<PathBuf> {
        let url_hash = format!("{:x}", md5::compute(url));
        Ok(Path::new(&self.cache_dir).join(url_hash).join("state.json"))
    }
    
    pub fn get_cache_file(&self, url: &str) -> Result<PathBuf> {
        let url_hash = format!("{:x}", md5::compute(url));
        Ok(Path::new(&self.cache_dir).join(url_hash).join("cache.data"))
    }
}

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = Config::new("cache".to_string());
}
