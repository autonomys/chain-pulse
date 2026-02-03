#![feature(iterator_try_collect)]

mod error;
mod storage;
mod types;
mod xdm;

use crate::error::Error;
use crate::storage::Db;
use crate::types::{ChainId, DomainId};
use clap::Parser;
use shared::subspace::Subspace;
use tokio::task::JoinSet;
use tracing::{Instrument, info_span};
use tracing_subscriber::EnvFilter;

/// Cli config for indexer.
#[derive(Debug, Parser)]
pub(crate) struct Cli {
    #[arg(long, default_value = "./indexer/migrations")]
    migrations_path: String,
    #[arg(long, default_value = "wss://rpc.mainnet.autonomys.xyz/ws")]
    consensus_rpc: String,
    #[arg(long, default_value = "wss://auto-evm.mainnet.autonomys.xyz/ws")]
    auto_evm_rpc: String,
    #[arg(
        long,
        default_value = "postgres://indexer:password@localhost:5434/indexer?sslmode=disable"
    )]
    db_uri: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let Cli {
        migrations_path,
        consensus_rpc,
        auto_evm_rpc,
        db_uri,
    } = Cli::parse();

    let _db = Db::new(&db_uri, &migrations_path).await?;

    let mut join_set: JoinSet<Result<(), Error>> = JoinSet::default();

    // start consensus tasks
    start_tasks(ChainId::Consensus, &mut join_set, &consensus_rpc).await?;

    // start auto evm tasks
    start_tasks(ChainId::Domain(DomainId(0)), &mut join_set, &auto_evm_rpc).await?;

    // no task in the join set should exit
    // if exits, it is a failure
    if let Some(update) = join_set.join_next().await {
        update??;
    }

    Ok(())
}

async fn start_tasks(
    chain: ChainId,
    join_set: &mut JoinSet<Result<(), Error>>,
    rpc: &str,
) -> Result<(), Error> {
    let span = match chain {
        ChainId::Consensus => info_span!("consensus"),
        ChainId::Domain(DomainId(0)) => info_span!("auto-evm"),
        _ => return Err(Error::Config(format!("Unknown Chain: {chain:?}"))),
    };
    let subspace = Subspace::new_from_url(rpc).await?;
    let updater = subspace.runtime_metadata_updater();
    join_set.spawn(
        async move { updater.perform_runtime_updates().await.map_err(Into::into) }
            .instrument(span.clone()),
    );

    join_set.spawn(
        {
            let stream = subspace.blocks_stream();
            async move { xdm::index_xdm(chain, stream).await }
        }
        .instrument(span.clone()),
    );

    // listen for all consensus blocks
    join_set.spawn(
        async move { subspace.listen_for_all_blocks().await.map_err(Into::into) }.instrument(span),
    );
    Ok(())
}
