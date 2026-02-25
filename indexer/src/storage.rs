use crate::api;
use crate::api::{BlockDetails, MaybeBlockDetails};
use crate::error::Error;
use crate::events::{Event, IncomingTransferSuccessful, OutgoingTransferInitiatedWithTransfer};
use crate::types::{ChainId, Location, Transfer, U128Compat, XdmMessageId};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
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

pub(crate) struct UpsertSharePrice {
    pub(crate) operator_id: i64,
    pub(crate) domain_id: i32,
    pub(crate) epoch_index: i64,
    pub(crate) share_price: Decimal,
    pub(crate) total_stake: u128,
    pub(crate) total_shares: u128,
    pub(crate) block_height: i64,
    pub(crate) block_time: DateTime<Utc>,
}

/// Row returned by share-price queries; field names match the
/// `OperatorEpochSharePriceRow` TypeScript interface in auto-portal.
#[derive(sqlx::FromRow, Serialize)]
pub(crate) struct SharePriceRow {
    pub(crate) operator_id: String,
    pub(crate) domain_id: String,
    pub(crate) epoch_index: i64,
    pub(crate) share_price: String,
    pub(crate) total_stake: String,
    pub(crate) total_shares: String,
    pub(crate) block_height: String,
    pub(crate) block_time: DateTime<Utc>,
}
pub(crate) struct UpsertOperator {
    pub(crate) operator_id: u64,
    pub(crate) domain_id: u32,
    pub(crate) owner_account: String,
    pub(crate) signing_key: String,
    pub(crate) minimum_nominator_stake: u128,
    pub(crate) nomination_tax: u8,
    pub(crate) status: String,
    pub(crate) total_stake: u128,
    pub(crate) total_shares: u128,
    pub(crate) total_storage_fee_deposit: u128,
    pub(crate) block_time: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
pub(crate) struct OperatorRow {
    pub(crate) operator_id: String,
    pub(crate) domain_id: String,
    pub(crate) owner_account: String,
    pub(crate) signing_key: String,
    pub(crate) minimum_nominator_stake: String,
    pub(crate) nomination_tax: i16,
    pub(crate) status: String,
    pub(crate) total_stake: String,
    pub(crate) total_shares: String,
    pub(crate) total_storage_fee_deposit: String,
    pub(crate) nominator_count: i64,
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
            where transfer_initiated_on_src_at is not null
            order by transfer_initiated_on_src_at desc limit $1
        "#,
        )
            .bind(limit as i64)
            .fetch_all(&*self.pool)
            .await?;

        Ok(transfers)
    }

    pub(crate) async fn upsert_epoch_share_price(&self, p: UpsertSharePrice) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO indexer.operator_epoch_share_prices
                (operator_id, domain_id, epoch_index, share_price,
                 total_stake, total_shares, block_height, block_time)
            VALUES ($1, $2, $3, $4::numeric(28,18), $5::numeric(39,0), $6::numeric(39,0), $7, $8)
            ON CONFLICT (operator_id, domain_id, epoch_index) DO UPDATE SET
                share_price  = EXCLUDED.share_price,
                total_stake  = EXCLUDED.total_stake,
                total_shares = EXCLUDED.total_shares,
                block_height = EXCLUDED.block_height,
                block_time   = EXCLUDED.block_time
            "#,
        )
        .bind(p.operator_id)
        .bind(p.domain_id)
        .bind(p.epoch_index)
        .bind(p.share_price.to_string())
        .bind(p.total_stake.to_string())
        .bind(p.total_shares.to_string())
        .bind(p.block_height)
        .bind(p.block_time)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn upsert_nominator(
        &self,
        operator_id: u64,
        address: &str,
        status: &str,
        block_height: u32,
    ) -> Result<(), Error> {
        // Upsert the nominator row, then refresh the materialized count on the
        // operators table so we never need a COUNT(*) at query time.
        sqlx::query(
            r#"
            WITH upsert AS (
                INSERT INTO indexer.nominators (operator_id, address, status, block_height)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (operator_id, address) DO UPDATE
                    SET status = EXCLUDED.status,
                        block_height = EXCLUDED.block_height
                    WHERE EXCLUDED.block_height >= indexer.nominators.block_height
                RETURNING operator_id
            )
            UPDATE indexer.operators
            SET nominator_count = (
                SELECT COUNT(*) FROM indexer.nominators
                WHERE operator_id = $1 AND status = 'active'
            )
            WHERE operator_id = $1
            "#,
        )
        .bind(operator_id as i64)
        .bind(address)
        .bind(status)
        .bind(block_height as i64)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_share_prices_latest(
        &self,
        operator_id: i64,
        limit: i64,
    ) -> Result<Vec<SharePriceRow>, Error> {
        let rows = sqlx::query_as::<_, SharePriceRow>(
            r#"
            SELECT operator_id::text, domain_id::text, epoch_index,
                   share_price::text, total_stake::text, total_shares::text,
                   block_height::text, block_time
            FROM indexer.operator_epoch_share_prices
            WHERE operator_id = $1
            ORDER BY block_time DESC
            LIMIT $2
            "#,
        )
        .bind(operator_id)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn get_share_prices_since(
        &self,
        operator_id: i64,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<SharePriceRow>, Error> {
        let rows = sqlx::query_as::<_, SharePriceRow>(
            r#"
            SELECT operator_id::text, domain_id::text, epoch_index,
                   share_price::text, total_stake::text, total_shares::text,
                   block_height::text, block_time
            FROM indexer.operator_epoch_share_prices
            WHERE operator_id = $1
              AND block_time >= $2
            ORDER BY block_time ASC
            LIMIT $3
            "#,
        )
        .bind(operator_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn get_share_prices_until(
        &self,
        operator_id: i64,
        until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<SharePriceRow>, Error> {
        let rows = sqlx::query_as::<_, SharePriceRow>(
            r#"
            SELECT operator_id::text, domain_id::text, epoch_index,
                   share_price::text, total_stake::text, total_shares::text,
                   block_height::text, block_time
            FROM indexer.operator_epoch_share_prices
            WHERE operator_id = $1
              AND block_time <= $2
            ORDER BY block_time DESC
            LIMIT $3
            "#,
        )
        .bind(operator_id)
        .bind(until)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn get_active_nominator_count(&self, operator_id: i64) -> Result<i64, Error> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM indexer.nominators
            WHERE operator_id = $1
              AND status = 'active'
            "#,
        )
        .bind(operator_id)
        .fetch_one(&*self.pool)
        .await?;
        Ok(count)
    }
    /// Upsert an operator row. On conflict only updates the mutable columns
    /// (stake/shares/status/updated_at). Static columns (signing_key, etc.)
    /// are preserved from the first insert.
    pub(crate) async fn upsert_operator(&self, op: UpsertOperator) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO indexer.operators (
                operator_id, domain_id, owner_account, signing_key,
                minimum_nominator_stake, nomination_tax, status,
                total_stake, total_shares, total_storage_fee_deposit, updated_at
            )
            VALUES ($1, $2, $3, $4, $5::numeric(39,0), $6, $7,
                    $8::numeric(39,0), $9::numeric(39,0), $10::numeric(39,0), $11)
            ON CONFLICT (operator_id) DO UPDATE SET
                status                    = EXCLUDED.status,
                total_stake               = EXCLUDED.total_stake,
                total_shares              = EXCLUDED.total_shares,
                total_storage_fee_deposit = EXCLUDED.total_storage_fee_deposit,
                updated_at                = EXCLUDED.updated_at
            "#,
        )
        .bind(op.operator_id as i64)
        .bind(op.domain_id as i32)
        .bind(&op.owner_account)
        .bind(&op.signing_key)
        .bind(op.minimum_nominator_stake.to_string())
        .bind(op.nomination_tax as i16)
        .bind(&op.status)
        .bind(op.total_stake.to_string())
        .bind(op.total_shares.to_string())
        .bind(op.total_storage_fee_deposit.to_string())
        .bind(op.block_time)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_operator_status(
        &self,
        operator_id: u64,
        status: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            r#"
            UPDATE indexer.operators
            SET status = $2, updated_at = NOW()
            WHERE operator_id = $1
            "#,
        )
        .bind(operator_id as i64)
        .bind(status)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_operator_stats(
        &self,
        operator_id: u64,
        status: &str,
        total_stake: u128,
        total_shares: u128,
        total_storage_fee_deposit: u128,
        block_time: DateTime<Utc>,
    ) -> Result<(), Error> {
        sqlx::query(
            r#"
            UPDATE indexer.operators
            SET status                    = $2,
                total_stake               = $3::numeric(39,0),
                total_shares              = $4::numeric(39,0),
                total_storage_fee_deposit = $5::numeric(39,0),
                updated_at                = $6
            WHERE operator_id = $1
            "#,
        )
        .bind(operator_id as i64)
        .bind(status)
        .bind(total_stake.to_string())
        .bind(total_shares.to_string())
        .bind(total_storage_fee_deposit.to_string())
        .bind(block_time)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_all_operators(&self) -> Result<Vec<OperatorRow>, Error> {
        let rows = sqlx::query_as::<_, OperatorRow>(
            r#"
            SELECT operator_id::text, domain_id::text, owner_account,
                   signing_key, minimum_nominator_stake::text, nomination_tax,
                   status, total_stake::text, total_shares::text,
                   total_storage_fee_deposit::text, nominator_count
            FROM indexer.operators
            ORDER BY operators.operator_id ASC
            "#,
        )
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn get_operator(
        &self,
        operator_id: i64,
    ) -> Result<Option<OperatorRow>, Error> {
        let row = sqlx::query_as::<_, OperatorRow>(
            r#"
            SELECT operator_id::text, domain_id::text, owner_account,
                   signing_key, minimum_nominator_stake::text, nomination_tax,
                   status, total_stake::text, total_shares::text,
                   total_storage_fee_deposit::text, nominator_count
            FROM indexer.operators
            WHERE operator_id = $1
            "#,
        )
        .bind(operator_id)
        .fetch_optional(&*self.pool)
        .await?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{get_db, sample_operator, sample_share_price};
    use crate::types::{ChainId, DomainId};
    use crate::xdm::extract_xdm_events_for_block;
    use chrono::{DateTime, Utc};
    use rust_decimal::Decimal;
    use shared::subspace::{HashAndNumber, Subspace};
    use sp_core::crypto::{Ss58AddressFormat, set_default_ss58_version};
    use std::str::FromStr;
    use subxt::utils::H256;

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

    #[tokio::test]
    async fn test_upsert_operator_insert_and_update() {
        let db = get_db().await;
        let op = sample_operator(1);
        db.db.upsert_operator(op).await.unwrap();

        let row = db.db.get_operator(1).await.unwrap().unwrap();
        assert_eq!(row.operator_id, "1");
        assert_eq!(row.domain_id, "0");
        assert_eq!(row.signing_key, "0xsigning_key_1");
        assert_eq!(row.status, "registered");
        assert_eq!(row.nomination_tax, 5);

        // Re-upsert with updated stats — static fields (signing_key, etc.) should be preserved
        let mut op2 = sample_operator(1);
        op2.total_stake = 100_000_000_000_000_000_000;
        op2.status = "deregistered".to_string();
        db.db.upsert_operator(op2).await.unwrap();

        let row = db.db.get_operator(1).await.unwrap().unwrap();
        assert_eq!(row.status, "deregistered");
        assert_eq!(row.total_stake, "100000000000000000000");
        // signing_key preserved from original insert
        assert_eq!(row.signing_key, "0xsigning_key_1");
    }

    #[tokio::test]
    async fn test_update_operator_status() {
        let db = get_db().await;
        db.db.upsert_operator(sample_operator(2)).await.unwrap();

        db.db
            .update_operator_status(2, "deregistered")
            .await
            .unwrap();
        let row = db.db.get_operator(2).await.unwrap().unwrap();
        assert_eq!(row.status, "deregistered");

        db.db.update_operator_status(2, "registered").await.unwrap();
        let row = db.db.get_operator(2).await.unwrap().unwrap();
        assert_eq!(row.status, "registered");
    }

    #[tokio::test]
    async fn test_update_operator_stats() {
        let db = get_db().await;
        db.db.upsert_operator(sample_operator(3)).await.unwrap();

        let new_time = DateTime::from_timestamp_millis(1_700_001_000_000).unwrap();
        db.db
            .update_operator_stats(3, "registered", 999, 888, 777, new_time)
            .await
            .unwrap();

        let row = db.db.get_operator(3).await.unwrap().unwrap();
        assert_eq!(row.total_stake, "999");
        assert_eq!(row.total_shares, "888");
        assert_eq!(row.total_storage_fee_deposit, "777");
    }

    #[tokio::test]
    async fn test_update_operator_stats_noop_for_missing_operator() {
        let db = get_db().await;
        // No operator 99 exists — update_operator_stats should be a silent no-op
        let result = db
            .db
            .update_operator_stats(
                99,
                "registered",
                1,
                1,
                1,
                DateTime::from_timestamp_millis(1_700_000_000_000).unwrap(),
            )
            .await;
        assert!(result.is_ok());
        assert!(db.db.get_operator(99).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_all_operators_sorted() {
        let db = get_db().await;
        // Insert operators out of order
        db.db.upsert_operator(sample_operator(5)).await.unwrap();
        db.db.upsert_operator(sample_operator(2)).await.unwrap();
        db.db.upsert_operator(sample_operator(10)).await.unwrap();

        let all = db.db.get_all_operators().await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].operator_id, "2");
        assert_eq!(all[1].operator_id, "5");
        assert_eq!(all[2].operator_id, "10");
    }

    #[tokio::test]
    async fn test_get_operator_not_found() {
        let db = get_db().await;
        let result = db.db.get_operator(999).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_upsert_nominator_block_height_guard() {
        let db = get_db().await;
        let addr = "st1234567890abcdef";

        // Block 10: nominator is active
        db.db.upsert_nominator(1, addr, "active", 10).await.unwrap();
        let count = db.db.get_active_nominator_count(1).await.unwrap();
        assert_eq!(count, 1);

        // Block 5 (older): withdrawal arrives out-of-order via buffered parallelism
        // This should NOT overwrite the block-10 "active" status
        db.db
            .upsert_nominator(1, addr, "withdrawn", 5)
            .await
            .unwrap();
        let count = db.db.get_active_nominator_count(1).await.unwrap();
        assert_eq!(count, 1, "older block should not overwrite newer status");

        // Block 15 (newer): withdrawal is legitimate
        db.db
            .upsert_nominator(1, addr, "withdrawn", 15)
            .await
            .unwrap();
        let count = db.db.get_active_nominator_count(1).await.unwrap();
        assert_eq!(count, 0, "newer block should update status to withdrawn");
    }

    #[tokio::test]
    async fn test_upsert_nominator_same_block_overwrites() {
        let db = get_db().await;
        let addr = "st_same_block";

        // Same block height: the later write wins (>= guard allows it)
        db.db.upsert_nominator(1, addr, "active", 10).await.unwrap();
        db.db
            .upsert_nominator(1, addr, "withdrawn", 10)
            .await
            .unwrap();
        let count = db.db.get_active_nominator_count(1).await.unwrap();
        assert_eq!(count, 0, "same-block write should update");
    }

    #[tokio::test]
    async fn test_get_active_nominator_count_multiple_operators() {
        let db = get_db().await;

        // Operator 1: 2 active, 1 withdrawn
        db.db
            .upsert_nominator(1, "addr_a", "active", 10)
            .await
            .unwrap();
        db.db
            .upsert_nominator(1, "addr_b", "active", 10)
            .await
            .unwrap();
        db.db
            .upsert_nominator(1, "addr_c", "withdrawn", 10)
            .await
            .unwrap();

        // Operator 2: 1 active
        db.db
            .upsert_nominator(2, "addr_a", "active", 10)
            .await
            .unwrap();

        assert_eq!(db.db.get_active_nominator_count(1).await.unwrap(), 2);
        assert_eq!(db.db.get_active_nominator_count(2).await.unwrap(), 1);
        assert_eq!(db.db.get_active_nominator_count(99).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_upsert_epoch_share_price_overwrites_on_reorg() {
        let db = get_db().await;

        // First write for operator 1, epoch 5
        let sp = sample_share_price(1, 5, 1000);
        db.db.upsert_epoch_share_price(sp).await.unwrap();

        let rows = db.db.get_share_prices_latest(1, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_stake, "50000000000000000000");

        // Re-org: same (operator, domain, epoch) but different data
        let mut sp2 = sample_share_price(1, 5, 1001);
        sp2.total_stake = 99_000_000_000_000_000_000;
        sp2.share_price = Decimal::new(1_050_000_000_000_000_000, 18); // 1.05
        db.db.upsert_epoch_share_price(sp2).await.unwrap();

        let rows = db.db.get_share_prices_latest(1, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].total_stake, "99000000000000000000",
            "re-org should overwrite"
        );
        assert_eq!(rows[0].block_height, "1001");
    }

    #[tokio::test]
    async fn test_get_share_prices_latest_with_limit() {
        let db = get_db().await;

        // Insert 5 epochs for operator 1
        for epoch in 1..=5 {
            let sp = sample_share_price(1, epoch, epoch * 100);
            db.db.upsert_epoch_share_price(sp).await.unwrap();
        }

        // Query latest 3
        let rows = db.db.get_share_prices_latest(1, 3).await.unwrap();
        assert_eq!(rows.len(), 3);
        // Ordered DESC by block_time → epochs 5, 4, 3
        assert_eq!(rows[0].epoch_index, 5);
        assert_eq!(rows[1].epoch_index, 4);
        assert_eq!(rows[2].epoch_index, 3);
    }

    #[tokio::test]
    async fn test_get_share_prices_since() {
        let db = get_db().await;

        for epoch in 1..=5 {
            let sp = sample_share_price(1, epoch, epoch * 100);
            db.db.upsert_epoch_share_price(sp).await.unwrap();
        }

        // `since` the time of epoch 3 (base + 3 * 60s)
        let since: DateTime<Utc> =
            DateTime::from_timestamp_millis(1_700_000_000_000 + 3 * 60_000).unwrap();
        let rows = db.db.get_share_prices_since(1, since, 50).await.unwrap();
        // Should return epochs 3, 4, 5 (ascending)
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].epoch_index, 3);
        assert_eq!(rows[1].epoch_index, 4);
        assert_eq!(rows[2].epoch_index, 5);
    }

    #[tokio::test]
    async fn test_get_share_prices_until() {
        let db = get_db().await;

        for epoch in 1..=5 {
            let sp = sample_share_price(1, epoch, epoch * 100);
            db.db.upsert_epoch_share_price(sp).await.unwrap();
        }

        // `until` the time of epoch 3
        let until: DateTime<Utc> =
            DateTime::from_timestamp_millis(1_700_000_000_000 + 3 * 60_000).unwrap();
        let rows = db.db.get_share_prices_until(1, until, 50).await.unwrap();
        // Should return epochs 3, 2, 1 (descending)
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].epoch_index, 3);
        assert_eq!(rows[1].epoch_index, 2);
        assert_eq!(rows[2].epoch_index, 1);
    }

    #[tokio::test]
    async fn test_share_prices_different_operators_isolated() {
        let db = get_db().await;

        db.db
            .upsert_epoch_share_price(sample_share_price(1, 1, 100))
            .await
            .unwrap();
        db.db
            .upsert_epoch_share_price(sample_share_price(1, 2, 200))
            .await
            .unwrap();
        db.db
            .upsert_epoch_share_price(sample_share_price(2, 1, 100))
            .await
            .unwrap();

        assert_eq!(db.db.get_share_prices_latest(1, 50).await.unwrap().len(), 2);
        assert_eq!(db.db.get_share_prices_latest(2, 50).await.unwrap().len(), 1);
        assert_eq!(
            db.db.get_share_prices_latest(99, 50).await.unwrap().len(),
            0
        );
    }
}
