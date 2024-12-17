use std::cmp::max;
use std::collections::BTreeMap;
use std::ops::Range;
use std::time::SystemTime;
use tokio::sync::RwLock;
use crate::utils::error::{Result, ProxyError};

/// 区块状态
#[derive(Debug, Clone, PartialEq)]
pub enum BlockState {
    Complete,    // 完成
    Downloading, // 下载中
    Pending,     // 等待下载
}

/// 区块信息
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub offset: u64,         // 起始位置
    pub length: u64,         // 长度
    pub state: BlockState,   // 状态
    pub last_access: SystemTime, // 最后访问时间
    pub priority: u32,       // 优先级
}

/// 区块管理器
#[derive(Debug)]
pub struct BlockManager {
    blocks: RwLock<BTreeMap<u64, BlockInfo>>, // 使用 BTreeMap 按偏移量排序存储区块
}

impl BlockManager {
    pub fn new() -> Self {
        Self {
            blocks: RwLock::new(BTreeMap::new()),
        }
    }

    /// 检查区块是否存在
    pub async fn check_range(&self, range: Range<u64>) -> Vec<Range<u64>> {
        let blocks = self.blocks.read().await;
        let mut missing_ranges = Vec::new();
        let mut current = range.start;

        // 遍历所有区块，找出缺失的范围
        for (offset, block) in blocks.range(..=range.end) {
            if current < *offset {
                // 当前位置到区块起始位置之间有缺失
                missing_ranges.push(current..*offset);
            }
            
            if block.state == BlockState::Complete {
                // 更新当前位置到已完成区块的结束位置
                current = max(current, offset + block.length);
            }
        }

        // 检查最后一段是否缺失
        if current < range.end {
            missing_ranges.push(current..range.end);
        }

        missing_ranges
    }

    /// 添加新区块
    pub async fn add_block(&self, offset: u64, length: u64, state: BlockState) -> Result<()> {
        let mut blocks = self.blocks.write().await;
        
        // 检查是否与现有区块重叠
        if let Some((_, existing)) = blocks.range(..=offset).next_back() {
            if offset < existing.offset + existing.length {
                return Err(ProxyError::Cache("区块重叠".to_string()));
            }
        }

        blocks.insert(offset, BlockInfo {
            offset,
            length,
            state,
            last_access: SystemTime::now(),
            priority: 0,
        });

        // 尝试合并相邻区块
        self.merge_blocks().await;
        Ok(())
    }

    /// 更新区块状态
    pub async fn update_block_state(&self, offset: u64, state: BlockState) -> Result<()> {
        let mut blocks = self.blocks.write().await;
        if let Some(block) = blocks.get_mut(&offset) {
            block.state = state;
            block.last_access = SystemTime::now();
            Ok(())
        } else {
            Err(ProxyError::Cache("区块不存在".to_string()))
        }
    }

    /// 合并相邻区块
    async fn merge_blocks(&self) {
        let mut blocks = self.blocks.write().await;
        let mut merged = Vec::new();
        let mut current_block: Option<BlockInfo> = None;

        // 收集需要合并的区块
        for (_, block) in blocks.iter() {
            if let Some(mut current) = current_block {
                if current.offset + current.length == block.offset 
                   && current.state == BlockState::Complete 
                   && block.state == BlockState::Complete {
                    // 可以合并
                    current.length += block.length;
                    current_block = Some(current);
                } else {
                    // 不能合并，保存当前区块
                    merged.push(current);
                    current_block = Some(block.clone());
                }
            } else {
                current_block = Some(block.clone());
            }
        }

        // 保存最后一个区块
        if let Some(block) = current_block {
            merged.push(block);
        }

        // 更新区块表
        blocks.clear();
        for block in merged {
            blocks.insert(block.offset, block);
        }
    }

    /// 获取下一个要下载的区块
    pub async fn get_next_pending_block(&self) -> Option<BlockInfo> {
        let mut blocks = self.blocks.write().await;
        for (_, block) in blocks.iter_mut() {
            if block.state == BlockState::Pending {
                block.state = BlockState::Downloading;
                return Some(block.clone());
            }
        }
        None
    }

    /// 清理过期区块
    pub async fn cleanup_expired_blocks(&self, max_age: std::time::Duration) {
        let mut blocks = self.blocks.write().await;
        let now = SystemTime::now();
        blocks.retain(|_, block| {
            if let Ok(age) = now.duration_since(block.last_access) {
                age < max_age
            } else {
                true
            }
        });
    }
} 