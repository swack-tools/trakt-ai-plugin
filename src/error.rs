use serde_json::{Value, json};
use worker::Response;
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub retry_after: Option<u64>,
}
impl ApiError {
    pub fn new(status: u16, code: &'static str) -> Self {
        Self {
            status,
            code,
            retry_after: None,
        }
    }
    pub fn value(&self) -> Value {
        json!({"error":self.code,"retry_after":self.retry_after})
    }
    pub fn response(&self) -> worker::Result<Response> {
        let mut r = Response::from_json(&self.value())?.with_status(self.status);
        if let Some(n) = self.retry_after {
            r.headers_mut().set("Retry-After", &n.to_string())?;
        }
        Ok(r)
    }
}
impl From<worker::Error> for ApiError {
    fn from(_: worker::Error) -> Self {
        Self::new(500, "storage_or_runtime_error")
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(_: serde_json::Error) -> Self {
        Self::new(400, "invalid_json")
    }
}
impl From<worker::kv::KvError> for ApiError {
    fn from(_: worker::kv::KvError) -> Self {
        Self::new(500, "cache_error")
    }
}
pub type Result<T> = std::result::Result<T, ApiError>;
