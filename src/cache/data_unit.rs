use std::cmp::{min, max};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataUnit {
    pub ranges: Vec<(u64, u64)>,         // 已缓存的数据范围
    pub last_accessed: DateTime<Utc>,     // 最后访问时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u64>,          // 文件总大小
    pub cache_file: String,               // 缓存文件路径
    pub allocated_size: u64,              // 已分配的文件大小
}

impl DataUnit {
    pub fn new(cache_file: String) -> Self {
        Self {
            ranges: Vec::new(),
            last_accessed: Utc::now(),
            total_size: None,
            cache_file,
            allocated_size: 0,
        }
    }

    // 添加新的缓存区间
    pub fn add_range(&mut self, start: u64, end: u64) {
        // 更新访问时间
        self.last_accessed = Utc::now();
        
        // 如果范围列表为空，直接添加
        if self.ranges.is_empty() {
            self.ranges.push((start, end));
            return;
        }

        // 尝试合并范围
        let mut new_ranges = Vec::new();
        let mut current_range = (start, end);
        let mut merged = false;

        for &range in &self.ranges {
            if Self::can_merge(current_range, range) {
                current_range = Self::merge_ranges(current_range, range);
                merged = true;
            } else if !merged && range.0 > current_range.1 {
                new_ranges.push(current_range);
                current_range = range;
                merged = true;
            } else if !merged && range.1 < current_range.0 {
                new_ranges.push(range);
            }
        }

        new_ranges.push(current_range);
        self.ranges = new_ranges;
        self.ranges.sort_by_key(|&(start, _)| start);
    }

    fn can_merge(range1: (u64, u64), range2: (u64, u64)) -> bool {
        range1.1 + 1 >= range2.0 || range2.1 + 1 >= range1.0
    }

    fn merge_ranges(range1: (u64, u64), range2: (u64, u64)) -> (u64, u64) {
        (
            std::cmp::min(range1.0, range2.0),
            std::cmp::max(range1.1, range2.1),
        )
    }

    // 检查是否包含指定区间
    pub fn contains_range(&self, start: u64, end: u64) -> bool {
        if end < start {
            return false;
        }

        for &(range_start, range_end) in &self.ranges {
            if start >= range_start && end <= range_end {
                return true;
            }
        }
        false
    }

    // 检查是否部分包含指定区间
    pub fn partially_contains_range(&self, start: u64, end: u64) -> bool {
        if end < start {
            return false;
        }

        for &(range_start, range_end) in &self.ranges {
            // 检查是否有任何重叠
            if !(end < range_start || start > range_end) {
                return true;
            }
        }
        false
    }

    // 获取已缓存的总大小
    pub fn get_cached_size(&self) -> u64 {
        self.ranges.iter().map(|&(s, e)| e - s + 1).sum()
    }

    // 检查是否完全缓存
    pub fn is_fully_cached(&self) -> bool {
        if let Some(total) = self.total_size {
            self.ranges.len() == 1 
                && self.ranges[0].0 == 0 
                && self.ranges[0].1 == total - 1
        } else {
            false
        }
    }

    // 检查是否需要扩展文件大小
    pub fn needs_allocation(&self, size: u64) -> bool {
        size > self.allocated_size
    }

    // 更新已分配大小
    pub fn update_allocated_size(&mut self, size: u64) {
        self.allocated_size = size;
    }

    // 设置文件总大小
    pub fn set_total_size(&mut self, size: u64) {
        self.total_size = Some(size);
    }

    // 更新访问时间
    pub fn update_access_time(&mut self) {
        self.last_accessed = Utc::now();
    }

    pub fn get_missing_ranges(&self, start: u64, end: u64) -> Vec<(u64, u64)> {
        let mut missing = Vec::new();
        let mut current = start;

        // 对范围进行排序
        let mut sorted_ranges = self.ranges.clone();
        sorted_ranges.sort_by_key(|&(start, _)| start);

        for &(range_start, range_end) in &sorted_ranges {
            if current < range_start {
                missing.push((current, range_start - 1));
            }
            current = range_end + 1;
        }

        if current <= end {
            missing.push((current, end));
        }

        missing
    }
}

impl Default for DataUnit {
    fn default() -> Self {
        Self::new(String::new())
    }
} 