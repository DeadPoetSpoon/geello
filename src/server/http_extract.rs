//! Sanitize extractor failures at the HTTP boundary. Raw deserialization errors
//! can contain caller values and internal type details; do not reflect them.
use super::ServerError;
use axum::{
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
};
use serde::de::DeserializeOwned;

pub(super) struct CheckedPath<T>(pub T);
pub(super) struct CheckedQuery<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for CheckedPath<T> {
    type Rejection = ServerError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|_| ServerError::invalid("Invalid path parameters"))
    }
}
impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for CheckedQuery<T> {
    type Rejection = ServerError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| ServerError::invalid("Invalid query parameters"))
    }
}
