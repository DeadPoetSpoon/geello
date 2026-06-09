use axum::response::{IntoResponse, Response};
use thiserror::Error;

use crate::GeelloError;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Render error: {0}")]
    RenderError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ServerError::BadRequest(m) => (axum::http::StatusCode::BAD_REQUEST, m),
            ServerError::NotFound(m) => (axum::http::StatusCode::NOT_FOUND, m),
            ServerError::RenderError(m) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, m),
            ServerError::Internal(m) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, msg).into_response()
    }
}

impl From<String> for ServerError {
    fn from(s: String) -> Self {
        ServerError::Internal(s)
    }
}

impl From<GeelloError> for ServerError {
    fn from(e: GeelloError) -> Self {
        ServerError::RenderError(e.to_string())
    }
}
