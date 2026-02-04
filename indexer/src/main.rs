#![feature(iterator_try_collect)]
#![deny(unused_crate_dependencies)]

mod error;
mod storage;
mod types;
mod xdm;

use crate::error::Error;
use crate::storage::Db;
use crate::types::{ChainId, DomainId};
use clap::Parser;
use shared::subspace::Subspace;
use sp_core::crypto::set_default_ss58_version;
use tokio::task::JoinSet;
use tracing::{Instrument, info_span};
use tracing_subscriber::EnvFilter;

/// Cli config for indexer.
#[derive(Debug, Parser)]
pub(crate) struct Cli {
    #[clap(long, env, default_value = "./indexer/migrations")]
    migrations_path: String,
    #[clap(long, env, default_value = "wss://rpc.mainnet.autonomys.xyz/ws")]
    consensus_rpc: String,
    #[clap(long, env, default_value = "wss://auto-evm.mainnet.autonomys.xyz/ws")]
    auto_evm_rpc: String,
    #[clap(
        long,
        env,
        default_value = "postgres://indexer:password@localhost:5434/indexer?sslmode=disable"
    )]
    db_uri: String,
    #[clap(long, env, default_value = "5000")]
    process_blocks_in_parallel: u32,
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
        process_blocks_in_parallel,
    } = Cli::parse();

    let db = Db::new(&db_uri, &migrations_path).await?;

    let mut join_set: JoinSet<Result<(), Error>> = JoinSet::default();

    // start consensus tasks
    start_tasks(
        ChainId::Consensus,
        &mut join_set,
        &consensus_rpc,
        &db,
        process_blocks_in_parallel,
    )
    .await?;

    // start auto evm tasks
    start_tasks(
        ChainId::Domain(DomainId(0)),
        &mut join_set,
        &auto_evm_rpc,
        &db,
        process_blocks_in_parallel,
    )
    .await?;

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
    db: &Db,
    process_blocks_in_parallel: u32,
) -> Result<(), Error> {
    let span = match chain {
        ChainId::Consensus => info_span!("consensus"),
        ChainId::Domain(DomainId(0)) => info_span!("auto-evm"),
        _ => return Err(Error::Config(format!("Unknown Chain: {chain:?}"))),
    };
    let subspace = Subspace::new_from_url(rpc).await?;
    let network_details = subspace.network_details().await?;
    set_default_ss58_version(network_details.ss58_format);
    let updater = subspace.runtime_metadata_updater();
    join_set.spawn(
        async move { updater.perform_runtime_updates().await.map_err(Into::into) }
            .instrument(span.clone()),
    );

    join_set.spawn(
        {
            let stream = subspace.blocks_stream();
            let block_provider = subspace.block_provider();
            let db = db.clone();
            async move {
                xdm::index_xdm(
                    chain,
                    stream,
                    block_provider,
                    db,
                    process_blocks_in_parallel,
                )
                .await
            }
        }
        .instrument(span.clone()),
    );

    // listen for all consensus blocks
    join_set.spawn(
        async move { subspace.listen_for_all_blocks().await.map_err(Into::into) }.instrument(span),
    );
    Ok(())
}
