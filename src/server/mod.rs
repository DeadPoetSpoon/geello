//! Feature-isolated server boundary. Library preparation/execution never imports
//! this module; resource ownership, HTTP, queues and caches live here only.
#[cfg(not(any(feature = "render-cpu", feature = "render-gpu")))]
compile_error!("the server feature requires render-cpu or render-gpu");
mod activity;
mod api;
mod archive;
mod catalog;
mod memory;
mod runtime;
mod scheduler;
mod settings;
pub use api::router;
pub use catalog::*;
pub use runtime::*;
pub use scheduler::*;
pub use settings::*;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct ServerError {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ResourceKey>,
}
impl ServerError {
    pub fn new(status: u16, kind: &str, title: impl Into<String>) -> Self {
        Self {
            kind: format!("urn:geello:problem:{kind}"),
            title: title.into(),
            status,
            references: vec![],
        }
    }
    pub(crate) fn invalid(title: impl Into<String>) -> Self {
        Self::new(400, "invalid-request", title)
    }
    pub(crate) fn conflict(title: impl Into<String>) -> Self {
        Self::new(409, "conflict", title)
    }
    pub(crate) fn io(_error: impl std::fmt::Display) -> Self {
        Self::new(500, "storage", "Resource storage operation failed")
    }
}
impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.title)
    }
}
impl std::error::Error for ServerError {}
impl From<crate::GeelloError> for ServerError {
    fn from(e: crate::GeelloError) -> Self {
        use crate::GeelloError::*;
        match e {
            Cancelled => Self::new(503, "cancelled", "Work was cancelled"),
            DeadlineExceeded => Self::new(503, "deadline", "Operation deadline expired"),
            BudgetExceeded(_) => Self::new(503, "budget", "Operation budget exhausted"),
            Io(_) => Self::new(422, "source", "Source could not be read"),
            Backend(_) | TargetInvalid(_) => Self::new(503, "backend", "Rendering backend failed"),
            _ => Self::new(422, "validation", e.to_string()),
        }
    }
}
impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self)).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
pub type Result<T> = std::result::Result<T, ServerError>;
