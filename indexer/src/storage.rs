use crate::api;
use crate::api::{BlockDetails, MaybeBlockDetails};
use crate::error::Error;
use crate::types::{
    ChainId, Event, IncomingTransferSuccessful, Location, OutgoingTransferInitiatedWithTransfer,
    Transfer, U128Compat, XdmMessageId,
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use shared::subspace::{BlockNumber, HashAndNumber};
use sqlx::PgPool;
use std::ops::Div;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use subxt::utils::to_hex;
use tracing::info;

pub(crate) async fn log_db_pool_info(db: Db, every: Duration) -> Result<(), Error> {
    let pool = db.pool.clone();
    let mut tick = tokio::time::interval(every);
    loop {
        tick.tick().await;

        let size = pool.size();
        let idle = pool.num_idle();
        let in_use = size.saturating_sub(idle as u32);
        let saturated = pool.try_acquire().is_none();
        let closed = pool.is_closed();

        info!(
            target: "db.pool",
            "size: {size}, idle: {idle}, in_use: {in_use}, saturated: {saturated}, closed: {closed}",
        );
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct XdmTransfer {
    src_chain: String,
    dst_chain: String,
    channel_id: String,
    nonce: String,
    sender: Option<String>,
    receiver: Option<String>,
    amount: Option<U128Compat>,
    transfer_initiated_block_number: Option<i64>,
    transfer_initiated_block_hash: Option<String>,
    transfer_initiated_on_src_at: Option<DateTime<Utc>>,
    transfer_executed_on_dst_block_number: Option<i64>,
    transfer_executed_on_dst_block_hash: Option<String>,
    transfer_executed_on_dst_at: Option<DateTime<Utc>>,
    transfer_acknowledged_on_src_block_number: Option<i64>,
    transfer_acknowledged_on_src_block_hash: Option<String>,
    transfer_acknowledged_on_src_at: Option<DateTime<Utc>>,
    transfer_successful: Option<bool>,
}

impl From<(Option<i64>, Option<String>, Option<DateTime<Utc>>)> for MaybeBlockDetails {
    fn from(value: (Option<i64>, Option<String>, Option<DateTime<Utc>>)) -> Self {
        if let (Some(block_number), Some(block_hash), Some(block_time)) = value {
            MaybeBlockDetails(Some(BlockDetails {
                block_number: block_number as BlockNumber,
                block_hash,
                block_time,
            }))
        } else {
            MaybeBlockDetails(None)
        }
    }
}

impl From<(Decimal, XdmTransfer)> for api::XdmTransfer {
    fn from(value: (Decimal, XdmTransfer)) -> Self {
        let (decimal_scale, transfer) = (value.0, value.1);
        let XdmTransfer {
            src_chain,
            dst_chain,
            channel_id,
            nonce,
            sender,
            receiver,
            amount,
            transfer_initiated_block_number,
            transfer_initiated_block_hash,
            transfer_initiated_on_src_at,
            transfer_executed_on_dst_block_number,
            transfer_executed_on_dst_block_hash,
            transfer_executed_on_dst_at,
            transfer_acknowledged_on_src_block_number,
            transfer_acknowledged_on_src_block_hash,
            transfer_acknowledged_on_src_at,
            transfer_successful,
        } = transfer;

        let amount = amount.map(|amount| Decimal::from(amount.0).div(decimal_scale));
        api::XdmTransfer {
            src_chain,
            dst_chain,
            channel_id,
            nonce,
            sender,
            receiver,
            amount,
            initiated_src_block: (
                transfer_initiated_block_number,
                transfer_initiated_block_hash,
                transfer_initiated_on_src_at,
            )
                .into(),
            executed_dst_block: (
                transfer_executed_on_dst_block_number,
                transfer_executed_on_dst_block_hash,
                transfer_executed_on_dst_at,
            )
                .into(),
            acknowledged_src_block: (
                transfer_acknowledged_on_src_block_number,
                transfer_acknowledged_on_src_block_hash,
                transfer_acknowledged_on_src_at,
            )
                .into(),
            transfer_successful,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Db {
    pub(crate) pool: Arc<PgPool>,
}

impl Db {
    pub(crate) async fn new(db_url: &str, migrations_path: &str) -> Result<Self, Error> {
        let pg_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(50)
            .acquire_slow_threshold(Duration::from_secs(10))
            .acquire_timeout(Duration::from_secs(60))
            .connect(db_url)
            .await?;

        let mut migrator = sqlx::migrate::Migrator::new(Path::new(migrations_path)).await?;
        migrator.set_ignore_missing(true);
        migrator.run(&pg_pool).await?;
        Ok(Db {
            pool: Arc::new(pg_pool),
        })
    }

    pub(crate) async fn set_last_processed_block(
        &self,
        process: &str,
        block_number: BlockNumber,
    ) -> Result<(), Error> {
        let _ = sqlx::query(
            r#"
        INSERT INTO indexer.metadata as m (process, processed_block_number)
        VALUES ($1, $2)
        ON CONFLICT (process) DO UPDATE
        SET processed_block_number = EXCLUDED.processed_block_number
        "#,
        )
        .bind(process)
        .bind(block_number as i64)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn get_last_processed_block(
        &self,
        process: &str,
    ) -> Result<BlockNumber, Error> {
        let number = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT processed_block_number
            FROM indexer.metadata
            WHERE process = $1
            "#,
        )
        .bind(process)
        .fetch_one(&*self.pool)
        .await?;

        Ok(number as BlockNumber)
    }

    pub(crate) async fn store_events(
        &self,
        src_chain: &ChainId,
        block: HashAndNumber,
        block_time: DateTime<Utc>,
        events: Vec<Event>,
    ) -> Result<(), Error> {
        for event in events {
            match event {
                Event::OutgoingTransferInitiated(transfer) => {
                    self.store_outgoing_transfer_initiated(&block, &block_time, transfer)
                        .await?
                }
                Event::OutgoingTransferFailed(transfer) => {
                    self.store_outgoing_transfer_acknowledgement(
                        &block,
                        src_chain,
                        transfer.chain_id,
                        &block_time,
                        transfer.message_id,
                        false,
                    )
                    .await?
                }
                Event::OutgoingTransferSuccessful(transfer) => {
                    self.store_outgoing_transfer_acknowledgement(
                        &block,
                        src_chain,
                        transfer.chain_id,
                        &block_time,
                        transfer.message_id,
                        true,
                    )
                    .await?
                }
                Event::IncomingTransferSuccessful(transfer) => {
                    self.store_incoming_transfer_execution(&block, src_chain, &block_time, transfer)
                        .await?
                }
            }
        }
        Ok(())
    }

    async fn store_incoming_transfer_execution(
        &self,
        block: &HashAndNumber,
        dst_chain: &ChainId,
        block_time: &DateTime<Utc>,
        transfer: IncomingTransferSuccessful,
    ) -> Result<(), Error> {
        let HashAndNumber { hash, number } = block;
        let IncomingTransferSuccessful {
            chain_id: src_chain,
            message_id,
            amount,
        } = transfer;
        let (channel_id, nonce) = (message_id.0, message_id.1);

        let query = sqlx::query(
            r#"
        insert into indexer.xdm_transfers (
            src_chain, dst_chain, channel_id, nonce, amount,
            transfer_executed_on_dst_block_number, transfer_executed_on_dst_block_hash, transfer_successful,
            transfer_executed_on_dst_at)
        values ($1, $2, $3::numeric(78, 0), $4::numeric(78,0), $5::numeric(39, 0), $6, $7, $8, $9)
        on conflict (src_chain, dst_chain, channel_id, nonce) do update
        set amount = excluded.amount,
            transfer_executed_on_dst_block_number = excluded.transfer_executed_on_dst_block_number,
            transfer_executed_on_dst_block_hash = excluded.transfer_executed_on_dst_block_hash,
            transfer_successful = excluded.transfer_successful,
            transfer_executed_on_dst_at = excluded.transfer_executed_on_dst_at
        "#,
        );

        let _ = query
            .bind(src_chain.to_string())
            .bind(dst_chain.to_string())
            .bind(channel_id.to_string())
            .bind(nonce.to_string())
            .bind(amount.to_string())
            .bind(*number as i64)
            .bind(to_hex(hash))
            .bind(true)
            .bind(block_time)
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    async fn store_outgoing_transfer_acknowledgement(
        &self,
        block: &HashAndNumber,
        src_chain: &ChainId,
        dst_chain: ChainId,
        block_time: &DateTime<Utc>,
        message_id: XdmMessageId,
        transfer_status: bool,
    ) -> Result<(), Error> {
        let HashAndNumber { hash, number } = block;

        let (channel_id, nonce) = (message_id.0, message_id.1);
        let query = sqlx::query(
            r#"
        insert into indexer.xdm_transfers (
            src_chain, dst_chain, channel_id, nonce, transfer_acknowledged_on_src_block_number,
            transfer_acknowledged_on_src_block_hash, transfer_successful, transfer_acknowledged_on_src_at)
        values ($1, $2, $3::numeric(78, 0), $4::numeric(78,0), $5, $6, $7, $8)
        on conflict (src_chain, dst_chain, channel_id, nonce) do update
        set transfer_acknowledged_on_src_block_hash = excluded.transfer_acknowledged_on_src_block_hash,
            transfer_acknowledged_on_src_block_number = excluded.transfer_acknowledged_on_src_block_number,
            transfer_successful = excluded.transfer_successful,
            transfer_acknowledged_on_src_at = excluded.transfer_acknowledged_on_src_at
        "#,
        );

        let _ = query
            .bind(src_chain.to_string())
            .bind(dst_chain.to_string())
            .bind(channel_id.to_string())
            .bind(nonce.to_string())
            .bind(*number as i64)
            .bind(to_hex(hash))
            .bind(transfer_status)
            .bind(block_time)
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    async fn store_outgoing_transfer_initiated(
        &self,
        initiated_block: &HashAndNumber,
        block_time: &DateTime<Utc>,
        transfer: OutgoingTransferInitiatedWithTransfer,
    ) -> Result<(), Error> {
        let OutgoingTransferInitiatedWithTransfer {
            message_id,
            transfer,
        } = transfer;

        let Transfer {
            amount,
            sender,
            receiver,
        } = transfer;

        let Location {
            chain_id: src_chain,
            account_id: sender,
        } = sender;

        let Location {
            chain_id: dst_chain,
            account_id: receiver,
        } = receiver;

        let (channel_id, nonce) = (message_id.0, message_id.1);

        let HashAndNumber { hash, number } = initiated_block;

        let query = sqlx::query(
            r#"
        insert into indexer.xdm_transfers (
            src_chain, dst_chain, channel_id, nonce, sender, receiver, amount,
            transfer_initiated_block_number, transfer_initiated_block_hash, transfer_initiated_on_src_at)
        values ($1, $2, $3::numeric(78, 0), $4::numeric(78,0), $5, $6, $7::numeric(39, 0), $8, $9, $10)
        on conflict (src_chain, dst_chain, channel_id, nonce) do update
        set sender = excluded.sender,
            receiver = excluded.receiver,
            amount = excluded.amount,
            transfer_initiated_block_number = excluded.transfer_initiated_block_number,
            transfer_initiated_block_hash = excluded.transfer_initiated_block_hash,
            transfer_initiated_on_src_at = excluded.transfer_initiated_on_src_at
        "#,
        );

        let _ = query
            .bind(src_chain.to_string())
            .bind(dst_chain.to_string())
            .bind(channel_id.to_string())
            .bind(nonce.to_string())
            .bind(sender.to_string())
            .bind(receiver.to_string())
            .bind(amount.to_string())
            .bind(*number as i64)
            .bind(to_hex(hash))
            .bind(block_time)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn get_xdm_transfer_for_address(
        &self,
        address: &str,
    ) -> Result<Vec<XdmTransfer>, Error> {
        let transfers = sqlx::query_as::<_, XdmTransfer>(
            r#"
            select src_chain, dst_chain, channel_id::text, nonce::text,
                   sender, receiver, amount::text,
                   transfer_initiated_block_number, transfer_initiated_block_hash, transfer_initiated_on_src_at,
                   transfer_executed_on_dst_block_number, transfer_executed_on_dst_block_hash, transfer_executed_on_dst_at,
                   transfer_acknowledged_on_src_block_number, transfer_acknowledged_on_src_block_hash, transfer_acknowledged_on_src_at,
                   transfer_successful from indexer.xdm_transfers
            where sender = $1 or receiver = $1 order by transfer_initiated_on_src_at desc
        "#,
        )
            .bind(address)
            .fetch_all(&*self.pool)
            .await?;

        Ok(transfers)
    }

    pub(crate) async fn get_recent_xdm_transfers(
        &self,
        limit: u64,
    ) -> Result<Vec<XdmTransfer>, Error> {
        let transfers = sqlx::query_as::<_, XdmTransfer>(
            r#"
            select src_chain, dst_chain, channel_id::text, nonce::text,
                   sender, receiver, amount::text,
                   transfer_initiated_block_number, transfer_initiated_block_hash, transfer_initiated_on_src_at,
                   transfer_executed_on_dst_block_number, transfer_executed_on_dst_block_hash, transfer_executed_on_dst_at,
                   transfer_acknowledged_on_src_block_number, transfer_acknowledged_on_src_block_hash, transfer_acknowledged_on_src_at,
                   transfer_successful from indexer.xdm_transfers
            order by transfer_initiated_on_src_at desc limit $1
        "#,
        )
            .bind(limit as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(transfers)
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::Db;
    use crate::types::{ChainId, DomainId};
    use crate::xdm::extract_xdm_events_for_block;
    use chrono::DateTime;
    use pgtemp::{PgTempDB, PgTempDBBuilder};
    use shared::subspace::{HashAndNumber, Subspace};
    use sp_core::crypto::{Ss58AddressFormat, set_default_ss58_version};
    use std::str::FromStr;
    use subxt::utils::H256;

    struct TestDb {
        db: Db,
        // used to hold the temp_db in context
        // else temp db is dropped and db timeouts since there
        // is no running postgres service underneath
        _temp_db: PgTempDB,
    }

    async fn get_db() -> TestDb {
        let temp_db = PgTempDBBuilder::new().start_async().await;
        let db = Db::new(temp_db.connection_uri().as_str(), "./migrations")
            .await
            .unwrap();

        TestDb {
            db,
            _temp_db: temp_db,
        }
    }

    #[tokio::test]
    async fn test_store_last_processed_block() {
        let db = get_db().await;
        let process = "test-process";
        let last_processed_block = db.db.get_last_processed_block(process).await;
        assert!(last_processed_block.is_err());

        let block = 100;
        db.db
            .set_last_processed_block(process, block)
            .await
            .unwrap();
        let last_processed_block = db.db.get_last_processed_block(process).await.unwrap();
        assert_eq!(last_processed_block, block);

        let block = 200;
        db.db
            .set_last_processed_block(process, block)
            .await
            .unwrap();
        let last_processed_block = db.db.get_last_processed_block(process).await.unwrap();
        assert_eq!(last_processed_block, block);
    }

    #[tokio::test]
    async fn test_store_consensus_outgoing_transfer_initiated() {
        set_default_ss58_version(Ss58AddressFormat::custom(6094));
        let db = get_db().await;
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap()
            .block_provider();

        let block_hash =
            H256::from_str("0x9e1d5eb5fddee84865824bb7b2c99c30573214f824a03a1a427843508bb6dad1")
                .unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let block = HashAndNumber {
            number: block_ext.number,
            hash: block_ext.hash,
        };
        let block_time =
            DateTime::from_timestamp_millis(block_ext.timestamp().await.unwrap() as i64).unwrap();
        db.db
            .store_events(&ChainId::Consensus, block, block_time, events)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_store_consensus_outgoing_transfer_failed() {
        let db = get_db().await;
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap()
            .block_provider();

        let block_hash =
            H256::from_str("0x950efc4f83b80076ba175723e206515c494ac9a3715209f2c6cc0b1111aca9c7")
                .unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let block = HashAndNumber {
            number: block_ext.number,
            hash: block_ext.hash,
        };
        let block_time =
            DateTime::from_timestamp_millis(block_ext.timestamp().await.unwrap() as i64).unwrap();
        db.db
            .store_events(&ChainId::Consensus, block, block_time, events)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_store_consensus_outgoing_transfer_successful() {
        let db = get_db().await;
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap()
            .block_provider();

        let block_hash =
            H256::from_str("0x09fc01ebf1791bd1e6f69d771e9672932cd450fd072cbf8fe4faeef100048343")
                .unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let block = HashAndNumber {
            number: block_ext.number,
            hash: block_ext.hash,
        };
        let block_time =
            DateTime::from_timestamp_millis(block_ext.timestamp().await.unwrap() as i64).unwrap();
        db.db
            .store_events(&ChainId::Consensus, block, block_time, events)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_store_consensus_incoming_transfer_successful() {
        let db = get_db().await;
        let subspace = Subspace::new_from_url("wss://rpc.mainnet.autonomys.xyz/ws")
            .await
            .unwrap()
            .block_provider();

        let block_hash =
            H256::from_str("0xcfcdfe0ab17288e67240d3d9d95074139b24d917c6f0352e2055e62001d4e92d")
                .unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = extract_xdm_events_for_block(&ChainId::Consensus, &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let block = HashAndNumber {
            number: block_ext.number,
            hash: block_ext.hash,
        };
        let block_time =
            DateTime::from_timestamp_millis(block_ext.timestamp().await.unwrap() as i64).unwrap();
        db.db
            .store_events(&ChainId::Consensus, block, block_time, events)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_store_evm_domain_outgoing_transfer_initiated() {
        set_default_ss58_version(Ss58AddressFormat::custom(6094));
        let db = get_db().await;
        let subspace = Subspace::new_from_url("wss://auto-evm.mainnet.autonomys.xyz/ws")
            .await
            .unwrap()
            .block_provider();

        let block_hash =
            H256::from_str("0xa3224142b5bf1ae57ed7f757f830806a0a153af701adfe52a5a740f3ede3aeea")
                .unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = extract_xdm_events_for_block(&ChainId::Domain(DomainId(0)), &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let block = HashAndNumber {
            number: block_ext.number,
            hash: block_ext.hash,
        };
        let block_time =
            DateTime::from_timestamp_millis(block_ext.timestamp().await.unwrap() as i64).unwrap();
        db.db
            .store_events(&ChainId::Domain(DomainId(0)), block, block_time, events)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_store_evm_domain_outgoing_transfer_successful() {
        let db = get_db().await;
        let subspace = Subspace::new_from_url("wss://auto-evm.mainnet.autonomys.xyz/ws")
            .await
            .unwrap()
            .block_provider();

        let block_hash =
            H256::from_str("0x823a47e998c0d699e52f50592136bc7f9f3807935a97bfd93196cce6242812ea")
                .unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = extract_xdm_events_for_block(&ChainId::Domain(DomainId(0)), &block_ext)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let block = HashAndNumber {
            number: block_ext.number,
            hash: block_ext.hash,
        };
        let block_time =
            DateTime::from_timestamp_millis(block_ext.timestamp().await.unwrap() as i64).unwrap();
        db.db
            .store_events(&ChainId::Domain(DomainId(0)), block, block_time, events)
            .await
            .unwrap();
    }
}
