//! Shared block-processing loop used by both `xdm` and `staking` processors.

use crate::error::Error;
use crate::storage::Db;
use futures_util::{StreamExt, TryStreamExt, stream};
use shared::subspace::{BlockNumber, BlocksStream, SubspaceBlockProvider};
use tokio::sync::broadcast::error::RecvError;
use tracing::info;

const CHECKPOINT_PROCESSED_BLOCK: u32 = 100;

/// Implemented by every block-level processor.
/// The outer stream loop is provided by [`run_processor`].
pub(crate) trait BlockProcessor {
    /// Called once per block; performs all DB writes for that block.
    async fn process_block(
        &self,
        block_number: BlockNumber,
        db: &Db,
        block_provider: &SubspaceBlockProvider,
    ) -> Result<(), Error>;

    /// The key used to checkpoint progress in `indexer.metadata`.
    fn checkpoint_key(&self) -> &str;

    /// Human-readable name for log messages.
    fn name(&self) -> &str;
}

/// Drives the stream loop for any [`BlockProcessor`] implementor.
pub(crate) async fn run_processor<P: BlockProcessor>(
    processor: P,
    mut stream: BlocksStream,
    block_provider: SubspaceBlockProvider,
    db: Db,
    process_blocks_in_parallel: u32,
) -> Result<(), Error> {
    let checkpoint_key = processor.checkpoint_key().to_string();
    let name = processor.name().to_string();
    loop {
        let blocks_ext = match stream.recv().await {
            Ok(block_ext) => block_ext,
            Err(RecvError::Lagged(_)) => continue,
            Err(err) => return Err(err.into()),
        };
        let last_processed_block_number = db
            .get_last_processed_block(&checkpoint_key)
            .await
            .unwrap_or(0);

        let (from, to) = if blocks_ext.blocks.len() == 1 {
            (
                last_processed_block_number + 1,
                blocks_ext
                    .blocks
                    .first()
                    .expect("must contain at least one block")
                    .number,
            )
        } else {
            let blocks = blocks_ext
                .blocks
                .iter()
                .map(|b| b.number)
                .collect::<Vec<_>>();
            let min = *blocks
                .iter()
                .min()
                .expect("should have more than one block");
            let max = *blocks
                .iter()
                .max()
                .expect("should have more than one block");
            (min.min(last_processed_block_number + 1), max)
        };

        if from > to {
            continue;
        }

        info!("{name}: indexing blocks from[{from}] to[{to}]...");
        let mut s = stream::iter((from..=to).map(|block| {
            let processor = &processor;
            let db = &db;
            let block_provider = &block_provider;
            async move {
                processor
                    .process_block(block, db, block_provider)
                    .await
                    .map(|_| block)
            }
        }))
        .buffered(process_blocks_in_parallel as usize);

        while let Some(block) = s.try_next().await? {
            if block.is_multiple_of(CHECKPOINT_PROCESSED_BLOCK) {
                info!("{name}: indexed block: {}", block);
                db.set_last_processed_block(&checkpoint_key, block).await?;
            }
        }

        info!("{name}: indexed block: {}", to);
        db.set_last_processed_block(&checkpoint_key, to).await?;
    }
}
