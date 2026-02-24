//! Shared test utilities for integration tests across modules.

use crate::storage::{Db, UpsertOperator, UpsertSharePrice};
use chrono::DateTime;
use pgtemp::{PgTempDB, PgTempDBBuilder};
use rust_decimal::Decimal;

pub(crate) struct TestDb {
    pub(crate) db: Db,
    // Hold the temp_db in scope — dropping it stops the ephemeral Postgres instance.
    pub(crate) _temp_db: PgTempDB,
}

pub(crate) async fn get_db() -> TestDb {
    let temp_db = PgTempDBBuilder::new().start_async().await;
    let db = Db::new(temp_db.connection_uri().as_str(), "./migrations")
        .await
        .unwrap();
    TestDb {
        db,
        _temp_db: temp_db,
    }
}

pub(crate) fn sample_operator(operator_id: u64) -> UpsertOperator {
    UpsertOperator {
        operator_id,
        domain_id: 0,
        owner_account: format!("owner_{operator_id}"),
        signing_key: format!("0xsigning_key_{operator_id}"),
        minimum_nominator_stake: 1_000_000_000_000_000_000,
        nomination_tax: 5,
        status: "registered".to_string(),
        total_stake: 50_000_000_000_000_000_000,
        total_shares: 50_000_000_000_000_000_000,
        total_storage_fee_deposit: 1_000_000_000_000_000_000,
        block_time: DateTime::from_timestamp_millis(1_700_000_000_000).unwrap(),
    }
}

pub(crate) fn sample_share_price(
    operator_id: i64,
    epoch_index: i64,
    block_height: i64,
) -> UpsertSharePrice {
    UpsertSharePrice {
        operator_id,
        domain_id: 0,
        epoch_index,
        share_price: Decimal::new(1_000_000_000_000_000_000, 18), // 1.0
        total_stake: 50_000_000_000_000_000_000,
        total_shares: 50_000_000_000_000_000_000,
        block_height,
        block_time: DateTime::from_timestamp_millis(1_700_000_000_000 + epoch_index * 60_000)
            .unwrap(),
    }
}
