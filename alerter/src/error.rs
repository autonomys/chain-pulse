use slack_morphism::errors::SlackClientError;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinError;

/// Overarching Error type for Alerter.
#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("Join error: {0}")]
    Join(#[from] JoinError),
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("RPC error: {0}")]
    Rpc(#[from] subxt_rpcs::Error),
    #[error("Scale error: {0}")]
    Scale(#[from] sp_runtime::codec::Error),
    #[error("Broadcast Receive error: {0}")]
    BroadRecvErr(#[from] RecvError),
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Slack error: {0}")]
    Slack(#[from] SlackClientError),
    #[error("Application error: {0}")]
    App(String),
    #[error("Subspace error: {0}")]
    Subspace(#[from] shared::error::Error),
}

impl From<subxt::Error> for Error {
    fn from(e: subxt::Error) -> Self {
        Self::Subspace(shared::error::Error::from(e))
    }
}
