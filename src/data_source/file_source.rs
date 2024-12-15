use crate::config::CONFIG;
use crate::utils::parse_range;
use crate::utils::error::Result;
use hyper::Body;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;
use bytes::Bytes;
use futures_util::stream::Stream;
use crate::utils::error::ProxyError;

pub struct FileSource {
    path: String,
    range: String,
}

impl FileSource {
    pub fn new(url: &str, range: &str) -> Self {
        let path = CONFIG.get_cache_path(url).to_string_lossy().into_owned();
        Self { path, range: range.to_string() }
    }

    pub async fn read_stream(&self) -> Result<impl Stream<Item = Result<Bytes>> + Send + 'static> {
        let (start, end) = parse_range(&self.range)?;
        let mut file = File::open(&self.path).await?;
        file.seek(SeekFrom::Start(start)).await?;
        
        let buffer_size = 64 * 1024; // 64KB 缓冲区
        let (mut sender, body) = Body::channel();
        let end_pos = end + 1;
        let mut current_pos = start;
        
        let mut file = Some(file);
        
        tokio::spawn(async move {
            let mut buffer = vec![0; buffer_size];
            while current_pos < end_pos {
                if let Some(ref mut f) = file {
                    let remaining = end_pos - current_pos;
                    let to_read = buffer_size.min(remaining as usize);
                    
                    match f.read(&mut buffer[..to_read]).await {
                        Ok(n) if n > 0 => {
                            current_pos += n as u64;
                            let chunk = buffer[..n].to_vec();
                            if let Err(e) = sender.send_data(Bytes::from(chunk)).await {
                                eprintln!("发送数据失败: {}", e);
                                break;
                            }
                        }
                        Ok(_) => break, // EOF
                        Err(e) => {
                            eprintln!("读取文件失败: {}", e);
                            break;
                        }
                    }
                }
            }
            file.take(); // 确保文件被关闭
        });

        let body_bytes = hyper::body::to_bytes(body).await?;
        let stream = futures_util::stream::once(async { Ok::<_, ProxyError>(body_bytes) });
        Ok(stream)
    }

    pub async fn read_data(&self) -> Result<Vec<u8>> {
        let mut file = File::open(&self.path).await?;
        let (start, end) = parse_range(&self.range)?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut buffer = vec![0; (end - start + 1) as usize];
        file.read_exact(&mut buffer).await?;
        Ok(buffer)
    }
}
