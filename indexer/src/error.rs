use sqlx::migrate::MigrateError;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinError;

/// Overarching Error type for Alerter.
#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("Config error: {0}")]
    Config(String),
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
