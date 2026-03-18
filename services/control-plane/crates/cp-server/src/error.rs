//! Error types and HTTP response mapping for the control-plane server.

#![cfg_attr(test, allow(clippy::expect_used))]

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use cp_api_types::ErrorResponse;
use thiserror::Error;

/// Application-level errors returned by handlers and state methods.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    DependencyUnavailable(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    #[must_use]
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::DependencyUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(ErrorResponse {
            error: self.to_string(),
        });
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;

    use super::AppError;

    #[test]
    fn validation_maps_to_400() {
        let response = AppError::Validation("bad input".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn not_found_maps_to_404() {
        let response = AppError::NotFound("missing".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn dependency_unavailable_maps_to_503() {
        let response =
            AppError::DependencyUnavailable("valkey unavailable".to_string()).into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn internal_maps_to_500() {
        let response = AppError::Internal(anyhow::anyhow!("boom")).into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
