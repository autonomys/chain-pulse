use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use sqlx::migrate::MigrateError;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinError;
use tracing::error;

/// Overarching Error type for Indexer.
#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("Config error: {0}")]
    Config(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[allow(dead_code)]
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Subspace error: {0}")]
    Subspace(#[from] shared::error::Error),
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Join error: {0}")]
    Join(#[from] JoinError),
    #[error("Broadcast Receive error: {0}")]
    BroadRecvErr(#[from] RecvError),
    #[error("DB error: {0}")]
    DB(#[from] sqlx::Error),
    #[error("Migrate error: {0}")]
    Migrate(#[from] MigrateError),
}

impl From<subxt::Error> for Error {
    fn from(e: subxt::Error) -> Self {
        Self::Subspace(shared::error::Error::from(e))
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let body = match self {
            Self::NotFound(msg) => {
                tracing::warn!("API 404: {msg}");
                format!("not found: {msg}")
            }
            Self::BadRequest(msg) => {
                tracing::warn!("API 400: {msg}");
                format!("bad request: {msg}")
            }
            other => {
                error!("API error: {other}");
                "internal server error".to_string()
            }
        };
        HttpResponse::build(self.status_code()).body(body)
    }
}
