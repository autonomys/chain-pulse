mod error;
mod xdm;

use crate::error::Error;
use clap::Parser;
use serde::Deserialize;
use shared::subspace::Subspace;
use std::collections::BTreeMap;
use std::fs;
use tokio::task::JoinSet;
use tracing::{Instrument, info, info_span};
use tracing_subscriber::EnvFilter;

/// Cli config for indexer.
#[derive(Debug, Parser)]
pub(crate) struct Cli {
    #[arg(long, default_value = "./indexer/config.toml")]
    config_path: String,
    #[arg(long, required = true)]
    network_name: String,
}

#[derive(Deserialize, Debug)]
struct Rpc {
    consensus_rpc: String,
    auto_evm_rpc: String,
}

#[derive(Deserialize, Debug)]
struct Config {
    rpc: BTreeMap<String, Rpc>,
}

enum Chain {
    Consensus,
    AutoEvm,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let Cli {
        config_path,
        network_name,
    } = Cli::parse();

    info!("Loading network configuration from `{config_path}`",);
    let config_data = fs::read_to_string(&config_path)?;
    let config = toml::from_str::<Config>(config_data.as_str())?;
    let Rpc {
        consensus_rpc,
        auto_evm_rpc,
    } = config.rpc.get(&network_name).ok_or(Error::Config(format!(
        "network `{network_name}` not found!"
    )))?;
    let mut join_set: JoinSet<Result<(), Error>> = JoinSet::default();

    // start consensus tasks
    start_tasks(Chain::Consensus, &mut join_set, consensus_rpc).await?;

    // start auto evm tasks
    start_tasks(Chain::AutoEvm, &mut join_set, auto_evm_rpc).await?;

    // no task in the join set should exit
    // if exits, it is a failure
    if let Some(update) = join_set.join_next().await {
        update??;
    }

    Ok(())
}

async fn start_tasks(
    span: Chain,
    join_set: &mut JoinSet<Result<(), Error>>,
    rpc: &str,
) -> Result<(), Error> {
    let span = match span {
        Chain::Consensus => info_span!("consensus"),
        Chain::AutoEvm => info_span!("auto-evm"),
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
            async move { xdm::index_xdm(stream).await }
        }
        .instrument(span.clone()),
    );

    // listen for all consensus blocks
    join_set.spawn(
        async move { subspace.listen_for_all_blocks().await.map_err(Into::into) }.instrument(span),
    );
    Ok(())
}
