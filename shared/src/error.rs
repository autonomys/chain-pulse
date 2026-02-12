use crate::subspace::BlockNumber;
use subxt::utils::H256;

/// Overarching Error type for Alerter.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Subxt error: {0}")]
    Subxt(Box<subxt::Error>),
    #[error("Block missing from backend")]
    MissingBlock,
    #[error("Block subscription closed")]
    SubscriptionClosed,
    #[error("RPC error: {0}")]
    Rpc(#[from] subxt_rpcs::Error),
    #[error("Block Hash missing from Cache")]
    MissingBlockHashFromCache(H256),
    #[error("Block body missing: {0}")]
    MissingBlockBody(H256),
    #[error("Block header missing for number: {0}")]
    MissingBlockHeaderForNumber(BlockNumber),
    #[error("Block header missing for hash: {0}")]
    MissingBlockHeaderForHash(H256),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Scale error: {0}")]
    Scale(#[from] sp_runtime::codec::Error),
    #[error("Config error: {0}")]
    Config(String),
}

impl From<subxt::Error> for Error {
    fn from(e: subxt::Error) -> Self {
        Self::Subxt(Box::new(e))
    }
}
