use crate::utils::parse_range;
use crate::utils::error::{Result, ProxyError};
use bytes::Bytes;
use futures_util::stream::Stream;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;
use std::pin::Pin;
use futures_util::Future;
use std::task::{Context, Poll};
use crate::{log_info, log_error};
use std::path::PathBuf;
use crate::CONFIG; // 导入 CONFIG

#[derive(Debug, Clone)]
pub struct FileSource {
    pub path: String,
    pub range: String,
}

impl FileSource {
    pub fn new(path: &str, range: &str) -> Self {
        Self {
            path: path.to_string(),
            range: range.to_string(),
        }
    }
    
    pub fn from_path_buf(path: Result<PathBuf>, range: &str) -> Result<Self> {
        let path_str = path?.to_string_lossy().into_owned();
        Ok(Self {
            path: path_str,
            range: range.to_string(),
        })
    }

    pub async fn read_stream(&self) -> Result<impl Stream<Item = Result<Bytes>>> {
        // 确保文件存在
        let cache_file = CONFIG.get_cache_file(&self.path)?;
        if !tokio::fs::try_exists(&cache_file).await? {
            return Err(ProxyError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "File not found")));
        }
        
        let file = tokio::fs::File::open(cache_file).await?;
        Ok(FileStream::new(file, self.range.clone()))
    }

    pub async fn read_data(&self) -> Result<Vec<u8>> {
        let mut file = File::open(&self.path).await?;
        let (start, end) = parse_range(&self.range)?;
        
        // 获取文件大小
        let file_size = file.metadata().await?.len();
        
        // 确保开始位置不超过文件大小
        if start >= file_size {
            return Err(ProxyError::Cache("请求范围超出文件大小".to_string()));
        }
        
        // 设置实际的结束位置
        let end_pos = std::cmp::min(end + 1, file_size);
        
        // 移动到起始位置
        file.seek(SeekFrom::Start(start)).await?;
        
        let mut buffer = vec![0; (end_pos - start) as usize];
        file.read_exact(&mut buffer).await?;
        Ok(buffer)
    }
}

pub struct FileStream {
    file: Option<File>,
    buffer_size: usize,
    current_pos: u64,
    end_pos: u64,
}

impl Stream for FileStream {
    type Item = Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 1. 检查是否已经读取完毕
        if self.current_pos > self.end_pos {
            return Poll::Ready(None);
        }

        // 2. 计算剩余需要读取的字节数
        let remaining = self.end_pos - self.current_pos + 1;
        let to_read = self.buffer_size.min(remaining as usize);
        let mut buffer = vec![0; to_read];

        // 3. 获取文件引用
        let file = if let Some(file) = self.file.as_mut() {
            file
        } else {
            return Poll::Ready(None);
        };

        // 4. 读取数据
        let read_future = file.read(&mut buffer);
        futures_util::pin_mut!(read_future);

        match read_future.poll(cx) {
            Poll::Ready(Ok(n)) if n > 0 => {
                let current = self.current_pos;
                buffer.truncate(n);
                self.current_pos += n as u64;
                log_info!("FileSource", "读取缓存: {} bytes at position {}", n, current);
                Poll::Ready(Some(Ok(Bytes::from(buffer))))
            }
            Poll::Ready(Ok(_)) => {
                self.file.take();
                Poll::Ready(None)
            }
            Poll::Ready(Err(e)) => {
                log_error!("FileSource", "读取文件失败: {}", e);
                self.file.take();
                Poll::Ready(Some(Err(ProxyError::Io(e))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl FileStream {
    pub fn new(file: File, range: String) -> Self {
        let (start, end) = parse_range(&range).expect("Invalid range");
        Self {
            file: Some(file),
            buffer_size: 1024, // 设置合适的缓冲区大小
            current_pos: start,
            end_pos: end,
        }
    }
}
