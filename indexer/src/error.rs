use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinError;

/// Overarching Error type for Alerter.
#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("Toml error: {0}")]
    Toml(#[from] toml::de::Error),
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
}

impl From<subxt::Error> for Error {
    fn from(e: subxt::Error) -> Self {
        Self::Subspace(shared::error::Error::from(e))
    }
}
