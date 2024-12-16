use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheState {
    pub cache_file: Option<PathBuf>,
    pub ranges: Vec<(u64, u64)>,
} 