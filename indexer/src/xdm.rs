use crate::error::Error;
use shared::subspace::BlocksStream;
use tracing::info;

pub(crate) async fn index_xdm(mut stream: BlocksStream) -> Result<(), Error> {
    loop {
        let blocks_ext = stream.recv().await?;
        for block in blocks_ext.blocks {
            info!("Indexing Block: {:?}", block.number);
        }
    }
}
