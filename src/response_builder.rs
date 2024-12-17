use hyper::{Body, Response, HeaderMap};
use bytes::Bytes;
use futures::Stream;
use crate::utils::error::Result;

pub struct ResponseBuilder;

impl ResponseBuilder {
    pub fn new() -> Self {
        Self
    }

    pub fn build_partial_content_response(
        &self,
        stream: Box<dyn Stream<Item = Result<Bytes>> + Send + Unpin>,
        headers: HeaderMap,
        start: u64,
        end: u64,
        total_size: u64,
    ) -> Response<Body> {
        let mut response = Response::new(Body::wrap_stream(stream));
        
        *response.status_mut() = hyper::StatusCode::PARTIAL_CONTENT;
        response.headers_mut().insert(
            hyper::header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, total_size).parse().unwrap()
        );
        response.headers_mut().insert(
            hyper::header::CONTENT_LENGTH,
            format!("{}", end - start + 1).parse().unwrap()
        );
        
        // 复制其他响应头
        for (key, value) in headers.iter() {
            response.headers_mut().insert(key, value.clone());
        }
        
        response
    }
} 