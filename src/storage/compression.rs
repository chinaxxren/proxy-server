use std::pin::Pin;
use bytes::Bytes;
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use futures::{Stream, StreamExt};
use pin_project_lite::pin_project;
use std::task::{Context, Poll};
use crate::utils::error::{Result, ProxyError};
use futures::stream::BoxStream;

impl From<flate2::CompressError> for ProxyError {
    fn from(err: flate2::CompressError) -> Self {
        ProxyError::Cache(err.to_string())
    }
}

impl From<flate2::DecompressError> for ProxyError {
    fn from(err: flate2::DecompressError) -> Self {
        ProxyError::Cache(err.to_string())
    }
}

pin_project! {
    pub struct CompressedStream<S> {
        #[pin]
        inner: S,
        compressor: Compress,
        buffer: Vec<u8>,
        temp_buffer: Vec<u8>,
    }
}

pin_project! {
    pub struct DecompressedStream<S> {
        #[pin]
        inner: S,
        decompressor: Decompress,
        buffer: Vec<u8>,
        temp_buffer: Vec<u8>,
    }
}

impl<S> CompressedStream<S>
where
    S: Stream<Item = Result<Bytes>>,
{
    pub fn new(inner: S, level: u32) -> Self {
        Self {
            inner,
            compressor: Compress::new(Compression::new(level), false),
            buffer: Vec::with_capacity(8192),
            temp_buffer: Vec::with_capacity(8192),
        }
    }

    pub fn boxed(self) -> BoxStream<'static, Result<Bytes>>
    where
        S: Send + 'static,
    {
        Box::pin(self)
    }
}

impl<S> DecompressedStream<S>
where
    S: Stream<Item = Result<Bytes>>,
{
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            decompressor: Decompress::new(false),
            buffer: Vec::with_capacity(8192),
            temp_buffer: Vec::with_capacity(8192),
        }
    }

    pub fn boxed(self) -> BoxStream<'static, Result<Bytes>>
    where
        S: Send + 'static,
    {
        Box::pin(self)
    }
}

impl<S> Stream for CompressedStream<S>
where
    S: Stream<Item = Result<Bytes>>,
{
    type Item = Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        
        match this.inner.poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.temp_buffer.clear();
                this.temp_buffer.extend_from_slice(&chunk);
                
                let mut compressed = false;
                loop {
                    match this.compressor.compress(
                        &this.temp_buffer,
                        this.buffer,
                        if compressed { FlushCompress::Finish } else { FlushCompress::None }
                    ) {
                        Ok(Status::Ok) | Ok(Status::BufError) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                            if !compressed {
                                compressed = true;
                            } else {
                                break;
                            }
                        }
                        Ok(Status::StreamEnd) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                            return Poll::Ready(None);
                        }
                        Err(e) => return Poll::Ready(Some(Err(ProxyError::Cache(e.to_string())))),
                    }
                }
                Poll::Ready(Some(Ok(Bytes::new())))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                this.temp_buffer.clear();
                loop {
                    match this.compressor.compress(
                        &[],
                        this.buffer,
                        FlushCompress::Finish
                    ) {
                        Ok(Status::Ok) | Ok(Status::BufError) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                        }
                        Ok(Status::StreamEnd) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                            return Poll::Ready(None);
                        }
                        Err(e) => return Poll::Ready(Some(Err(ProxyError::Cache(e.to_string())))),
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Stream for DecompressedStream<S>
where
    S: Stream<Item = Result<Bytes>>,
{
    type Item = Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        
        match this.inner.poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.temp_buffer.clear();
                this.temp_buffer.extend_from_slice(&chunk);
                
                let mut decompressed = false;
                loop {
                    match this.decompressor.decompress(
                        &this.temp_buffer,
                        this.buffer,
                        if decompressed { FlushDecompress::Finish } else { FlushDecompress::None }
                    ) {
                        Ok(Status::Ok) | Ok(Status::BufError) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                            if !decompressed {
                                decompressed = true;
                            } else {
                                break;
                            }
                        }
                        Ok(Status::StreamEnd) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                            return Poll::Ready(None);
                        }
                        Err(e) => return Poll::Ready(Some(Err(ProxyError::Cache(e.to_string())))),
                    }
                }
                Poll::Ready(Some(Ok(Bytes::new())))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                this.temp_buffer.clear();
                loop {
                    match this.decompressor.decompress(
                        &[],
                        this.buffer,
                        FlushDecompress::Finish
                    ) {
                        Ok(Status::Ok) | Ok(Status::BufError) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                        }
                        Ok(Status::StreamEnd) => {
                            if this.buffer.len() > 0 {
                                let result = Bytes::from(std::mem::take(this.buffer));
                                return Poll::Ready(Some(Ok(result)));
                            }
                            return Poll::Ready(None);
                        }
                        Err(e) => return Poll::Ready(Some(Err(ProxyError::Cache(e.to_string())))),
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[allow(dead_code)]
pub async fn calculate_checksum<S>(mut stream: S) -> Result<u32>
where
    S: Stream<Item = Result<Bytes>> + Unpin,
{
    let mut hasher = crc32fast::Hasher::new();
    
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
    }
    
    Ok(hasher.finalize())
} 