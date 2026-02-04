use crate::error::Error;
use crate::types::{
    ChainId, DomainId, Event, IncomingTransferSuccessful, OutgoingTransferFailed,
    OutgoingTransferInitiated, OutgoingTransferInitiatedWithTransfer, OutgoingTransferSuccessful,
    Transfer,
};
use futures_util::{StreamExt, TryStreamExt, stream};
use shared::subspace::{BlockExt, BlocksStream};
use subxt::SubstrateConfig;
use subxt::events::{EventDetails, StaticEvent};
use subxt::storage::StaticStorageKey;
use tracing::info;

pub(crate) async fn index_xdm(chain: ChainId, mut stream: BlocksStream) -> Result<(), Error> {
    loop {
        let blocks_ext = stream.recv().await?;
        for block in blocks_ext.blocks {
            info!("Indexing Block: {:?}", block.number);
            extract_xdm_events_for_block(&chain, &block).await?;
        }
    }
}

pub(crate) async fn extract_xdm_events_for_block(
    chain: &ChainId,
    block_ext: &BlockExt,
) -> Result<Vec<Event>, Error> {
    let block_events = match chain {
        ChainId::Consensus => block_ext
            .events()
            .await?
            .iter()
            .filter_map(|event| event.ok())
            .collect::<Vec<_>>(),
        ChainId::Domain(DomainId(0)) => block_ext.events_from_segments().await?,
        _ => return Err(Error::Config(format!("invalid chain id: {chain:?}"))),
    };
    let mut events: Vec<Event> = vec![];
    events.extend(as_events::<OutgoingTransferFailed>(&block_events)?);
    events.extend(as_events::<OutgoingTransferSuccessful>(&block_events)?);
    events.extend(as_events::<IncomingTransferSuccessful>(&block_events)?);
    let transfer_initiated_events = block_events
        .iter()
        .filter_map(|e| e.as_event::<OutgoingTransferInitiated>().ok().flatten())
        .collect::<Vec<_>>();

    let outgoing_init_events = stream::iter(transfer_initiated_events.into_iter().map(
        |event| async move {
            let OutgoingTransferInitiated {
                chain_id,
                message_id,
                amount: _,
            } = event;

            let k1 = StaticStorageKey::new(chain_id);
            let k2 = StaticStorageKey::new(message_id);
            block_ext
                .read_storage::<_, Transfer>("Transporter", "OutgoingTransfers", (k1, k2))
                .await
                .map(|transfer| {
                    Event::OutgoingTransferInitiated(OutgoingTransferInitiatedWithTransfer {
                        message_id,
                        transfer,
                    })
                })
        },
    ))
    .buffered(10)
    .try_collect::<Vec<_>>()
    .await?;
    events.extend(outgoing_init_events);
    Ok(events)
}

fn as_events<E: StaticEvent + Into<Event>>(
    block_events: &[EventDetails<SubstrateConfig>],
) -> Result<Vec<Event>, Error> {
    Ok(block_events
        .iter()
        .filter_map(|event| event.as_event::<E>().ok().flatten().map(Into::into))
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use crate::types::{
        ChainId, DomainId, Event, IncomingTransferSuccessful, Location, MultiAccountId,
        OutgoingTransferFailed, OutgoingTransferInitiatedWithTransfer, OutgoingTransferSuccessful,
        Transfer,
    };
    use crate::xdm::extract_xdm_events_for_block;
    use hex_literal::hex;
    use scale_decode::ext::primitive_types::U256;
    use shared::subspace::Subspace;
    use std::str::FromStr;
    use subxt::utils::{AccountId32, H256};

    #[tokio::test]
    async fn test_consensus_outgoing_transfer_initiated() {
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap();

        let block_hash =
            H256::from_str("0x9e1d5eb5fddee84865824bb7b2c99c30573214f824a03a1a427843508bb6dad1")
                .unwrap();
        let block_ext = subspace.block_ext(block_hash).await.unwrap();
        let mut events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        assert_eq!(
            event,
            Event::OutgoingTransferInitiated(OutgoingTransferInitiatedWithTransfer {
                message_id: (U256::zero().into(), U256::one().into()),
                transfer: Transfer {
                    amount: 1000000000000000000,
                    sender: Location {
                        chain_id: ChainId::Consensus,
                        account_id: MultiAccountId::AccountId32(
                            AccountId32::from_str(
                                "sucPReEfVCRPV1cQB3o2N83yPqJBjnUpAfbS5eFWeu6amCJcE"
                            )
                            .unwrap()
                        )
                    },
                    receiver: Location {
                        chain_id: ChainId::Domain(DomainId(0)),
                        account_id: MultiAccountId::AccountId20(
                            hex!("6febb20d01fc1b22dbf15e67a58fa85fa5f64c8d").into()
                        )
                    },
                },
            })
        )
    }

    #[tokio::test]
    async fn test_consensus_outgoing_transfer_failed() {
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap();

        let block_hash =
            H256::from_str("0x950efc4f83b80076ba175723e206515c494ac9a3715209f2c6cc0b1111aca9c7")
                .unwrap();
        let block_ext = subspace.block_ext(block_hash).await.unwrap();
        let mut events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        assert_eq!(
            event,
            Event::OutgoingTransferFailed(OutgoingTransferFailed {
                chain_id: ChainId::Domain(DomainId(0)),
                message_id: (U256::zero().into(), U256::from(34).into()),
            })
        )
    }

    #[tokio::test]
    async fn test_consensus_outgoing_transfer_successful() {
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap();

        let block_hash =
            H256::from_str("0x09fc01ebf1791bd1e6f69d771e9672932cd450fd072cbf8fe4faeef100048343")
                .unwrap();
        let block_ext = subspace.block_ext(block_hash).await.unwrap();
        let mut events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        assert_eq!(
            event,
            Event::OutgoingTransferSuccessful(OutgoingTransferSuccessful {
                chain_id: ChainId::Domain(DomainId(0)),
                message_id: (U256::zero().into(), U256::one().into()),
            })
        )
    }

    #[tokio::test]
    async fn test_consensus_incoming_transfer_successful() {
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap();

        let block_hash =
            H256::from_str("0xcfcdfe0ab17288e67240d3d9d95074139b24d917c6f0352e2055e62001d4e92d")
                .unwrap();
        let block_ext = subspace.block_ext(block_hash).await.unwrap();
        let mut events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        assert_eq!(
            event,
            Event::IncomingTransferSuccessful(IncomingTransferSuccessful {
                chain_id: ChainId::Domain(DomainId(0)),
                message_id: (U256::zero().into(), U256::zero().into()),
                amount: 1000000000000000000,
            })
        )
    }

    #[tokio::test]
    async fn test_evm_domain_outgoing_transfer_initiated() {
        let subspace = Subspace::new_from_url("wss://auto-evm.mainnet.autonomys.xyz/ws")
            .await
            .unwrap();

        let block_hash =
            H256::from_str("0xa3224142b5bf1ae57ed7f757f830806a0a153af701adfe52a5a740f3ede3aeea")
                .unwrap();
        let block_ext = subspace.block_ext(block_hash).await.unwrap();
        let mut events = extract_xdm_events_for_block(&ChainId::Domain(DomainId(0)), &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        assert_eq!(
            event,
            Event::OutgoingTransferInitiated(OutgoingTransferInitiatedWithTransfer {
                message_id: (U256::zero().into(), U256::from(102).into()),
                transfer: Transfer {
                    amount: 66400000000000000000000,
                    sender: Location {
                        chain_id: ChainId::Domain(DomainId(0)),
                        account_id: MultiAccountId::AccountId20(
                            hex!("ba36130e55c8c794c6829f4b2843f78b52f34dfe").into()
                        )
                    },
                    receiver: Location {
                        chain_id: ChainId::Consensus,
                        account_id: MultiAccountId::AccountId32([0; 32].into())
                    },
                },
            })
        )
    }

    #[tokio::test]
    async fn test_evm_domain_outgoing_transfer_successful() {
        let subspace = Subspace::new_from_url("wss://auto-evm.mainnet.autonomys.xyz/ws")
            .await
            .unwrap();

        let block_hash =
            H256::from_str("0x823a47e998c0d699e52f50592136bc7f9f3807935a97bfd93196cce6242812ea")
                .unwrap();
        let block_ext = subspace.block_ext(block_hash).await.unwrap();
        let mut events = extract_xdm_events_for_block(&ChainId::Domain(DomainId(0)), &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        assert_eq!(
            event,
            Event::OutgoingTransferSuccessful(OutgoingTransferSuccessful {
                chain_id: ChainId::Consensus,
                message_id: (U256::zero().into(), U256::from(102).into()),
            })
        )
    }
}
