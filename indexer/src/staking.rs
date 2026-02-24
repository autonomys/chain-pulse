use crate::error::Error;
use crate::events::{
    DomainEpochCompleted, NominatorUnlocked, OperatorDeactivated, OperatorDeregistered,
    OperatorNominated, OperatorReactivated, OperatorRegistered, OperatorSlashed, WithdrewStake,
};
use crate::processor::{self, BlockProcessor};
use crate::rpc_types::FullOperator;
use crate::storage::{Db, UpsertOperator, UpsertSharePrice};
use crate::types::{DomainEpoch, StakingSummary};
use chrono::DateTime;
use rust_decimal::Decimal;
use shared::subspace::{BlockNumber, BlocksStream, SubspaceBlockProvider};
use subxt::storage::StaticStorageKey;
use subxt::utils::AccountId32 as SubxtAccountId32;

pub(crate) const CHECKPOINT_KEY: &str = "staking_processor_Consensus";

pub(crate) struct StakingProcessor;

impl BlockProcessor for StakingProcessor {
    async fn process_block(
        &self,
        block_number: BlockNumber,
        db: &Db,
        block_provider: &SubspaceBlockProvider,
    ) -> Result<(), Error> {
        index_staking_for_block(block_number, db, block_provider).await
    }

    fn checkpoint_key(&self) -> &str {
        CHECKPOINT_KEY
    }

    fn name(&self) -> &str {
        "Staking"
    }
}

pub(crate) async fn index_staking(
    stream: BlocksStream,
    block_provider: SubspaceBlockProvider,
    db: Db,
    process_blocks_in_parallel: u32,
) -> Result<(), Error> {
    processor::run_processor(
        StakingProcessor,
        stream,
        block_provider,
        db,
        process_blocks_in_parallel,
    )
    .await
}

async fn index_staking_for_block(
    block_number: BlockNumber,
    db: &Db,
    block_provider: &SubspaceBlockProvider,
) -> Result<(), Error> {
    let block_ext = block_provider.block_ext_at_number(block_number).await?;
    let block_time = DateTime::from_timestamp_millis(block_ext.timestamp().await? as i64)
        .expect("should always be a valid Unix epoch time");
    let events = block_ext.events().await?;

    for event in events.find::<OperatorRegistered>() {
        let e = event?;
        let op: FullOperator = match block_ext
            .read_storage("Domains", "Operators", StaticStorageKey::new(e.operator_id))
            .await
        {
            Ok(op) => op,
            Err(_) => continue,
        };
        let owner: SubxtAccountId32 = match block_ext
            .read_storage(
                "Domains",
                "OperatorIdOwner",
                StaticStorageKey::new(e.operator_id),
            )
            .await
        {
            Ok(owner) => owner,
            Err(_) => continue,
        };
        let owner_account = sp_core::crypto::AccountId32::new(owner.0).to_string();
        let signing_key_hex = format!("0x{}", hex::encode(op.signing_key));
        db.upsert_operator(UpsertOperator {
            operator_id: e.operator_id,
            domain_id: op.current_domain_id,
            owner_account,
            signing_key: signing_key_hex,
            minimum_nominator_stake: op.minimum_nominator_stake,
            nomination_tax: op.nomination_tax,
            status: op.status.as_str().to_string(),
            total_stake: op.current_total_stake,
            total_shares: op.current_total_shares,
            total_storage_fee_deposit: op.total_storage_fee_deposit,
            block_time,
        })
        .await?;
    }

    for event in events.find::<OperatorDeregistered>() {
        let e = event?;
        db.update_operator_status(e.operator_id, "deregistered")
            .await?;
    }

    for event in events.find::<OperatorSlashed>() {
        let e = event?;
        db.update_operator_status(e.operator_id, "slashed").await?;
    }

    for event in events.find::<OperatorDeactivated>() {
        let e = event?;
        db.update_operator_status(e.operator_id, "deactivated")
            .await?;
    }

    for event in events.find::<OperatorReactivated>() {
        let e = event?;
        db.update_operator_status(e.operator_id, "registered")
            .await?;
    }

    for event in events.find::<OperatorNominated>() {
        let e = event?;
        let address = sp_core::crypto::AccountId32::new(e.nominator_id.0).to_string();
        db.upsert_nominator(e.operator_id, &address, "active", block_number)
            .await?;
    }

    for event in events.find::<WithdrewStake>() {
        let e = event?;
        let address = sp_core::crypto::AccountId32::new(e.nominator_id.0).to_string();
        db.upsert_nominator(e.operator_id, &address, "withdrawn", block_number)
            .await?;
    }

    for event in events.find::<NominatorUnlocked>() {
        let e = event?;
        let address = sp_core::crypto::AccountId32::new(e.nominator_id.0).to_string();
        db.upsert_nominator(e.operator_id, &address, "withdrawn", block_number)
            .await?;
    }

    for event in events.find::<DomainEpochCompleted>() {
        let e = event?;
        index_epoch_share_prices(&block_ext, &e, block_number, block_time, db).await?;
    }

    Ok(())
}

async fn index_epoch_share_prices(
    block_ext: &shared::subspace::BlockExt,
    epoch_event: &DomainEpochCompleted,
    block_height: BlockNumber,
    block_time: DateTime<chrono::Utc>,
    db: &Db,
) -> Result<(), Error> {
    let domain_id = epoch_event.domain_id.clone();
    let epoch_index = epoch_event.completed_epoch_index;

    // Fetch the staking summary for this domain to get the active operator set.
    let summary: StakingSummary = block_ext
        .read_storage(
            "Domains",
            "DomainStakingSummary",
            StaticStorageKey::new(domain_id.clone()),
        )
        .await?;

    for operator_id in summary.current_operators.keys() {
        let domain_epoch = DomainEpoch(domain_id.clone(), epoch_index);

        // Read the share price for this operator + epoch (OptionQuery — may be absent).
        let share_price_raw = match block_ext
            .read_storage::<_, u64>(
                "Domains",
                "OperatorEpochSharePrice",
                (
                    StaticStorageKey::new(*operator_id),
                    StaticStorageKey::new(domain_epoch),
                ),
            )
            .await
        {
            Ok(price) => price,
            Err(_) => continue,
        };

        // Read full operator data for stake/shares/status/storage-fee.
        let op: FullOperator = match block_ext
            .read_storage("Domains", "Operators", StaticStorageKey::new(*operator_id))
            .await
        {
            Ok(op) => op,
            Err(_) => continue,
        };

        // Convert Perquintill to decimal: raw_u64 / 10^18
        let share_price =
            Decimal::from(share_price_raw) / Decimal::from(1_000_000_000_000_000_000u64);

        db.upsert_epoch_share_price(UpsertSharePrice {
            operator_id: *operator_id as i64,
            domain_id: domain_id.0 as i32,
            epoch_index: epoch_index as i64,
            share_price,
            total_stake: op.current_total_stake,
            total_shares: op.current_total_shares,
            block_height: block_height as i64,
            block_time,
        })
        .await?;

        // Update operator stats in the operators table (only mutable fields).
        // This won't insert a new row — only updates existing ones indexed via
        // OperatorRegistered. If operator isn't in DB yet, this is a no-op.
        db.update_operator_stats(
            *operator_id,
            op.status.as_str(),
            op.current_total_stake,
            op.current_total_shares,
            op.total_storage_fee_deposit,
            block_time,
        )
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{
        DomainEpochCompleted, OperatorNominated, OperatorRegistered, WithdrewStake,
    };
    use crate::rpc_types::FullOperator;
    use crate::storage::{Db, UpsertOperator};
    use pgtemp::{PgTempDB, PgTempDBBuilder};
    use shared::subspace::Subspace;
    use std::str::FromStr;
    use subxt::storage::StaticStorageKey;
    use subxt::utils::{AccountId32 as SubxtAccountId32, H256};

    const RPC_URL: &str = "wss://rpc.mainnet.autonomys.xyz/ws";

    // Block hashes containing known staking events
    const OPERATOR_REGISTERED_HASH: &str =
        "0x9749ce3959c6e613a85f1576331ebb138aa3f00492dcde3b37ae978bb399c364";
    const DOMAIN_EPOCH_COMPLETED_HASH: &str =
        "0xb26dc651dd8317b593775da3202061dd0c1dea817e0e60c5f0f4b14c6f9efb39";
    const WITHDREW_STAKE_HASH: &str =
        "0xe70e7da10ae1fa68f5e274e6f673ae6933386e688284186dec88fc2870f38e24";
    const OPERATOR_NOMINATED_HASH: &str =
        "0x5df7664c6e14422fdbbc68f5d78f4252e2151bce887b9659e4eea53df59e3f74";

    struct TestDb {
        db: Db,
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

    fn sample_operator(operator_id: u64) -> UpsertOperator {
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
            block_time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap(),
        }
    }

    // ── Event-only decode tests (RPC, no DB) ─────────────────────────

    #[tokio::test]
    async fn test_decode_operator_registered() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = H256::from_str(OPERATOR_REGISTERED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = block_ext.events().await.unwrap();

        let registered: Vec<_> = events
            .find::<OperatorRegistered>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !registered.is_empty(),
            "should find OperatorRegistered event"
        );
        let e = &registered[0];
        assert!(e.operator_id > 0, "operator_id should be > 0");

        // Verify full storage decode path: read FullOperator + OperatorIdOwner
        let op: FullOperator = block_ext
            .read_storage("Domains", "Operators", StaticStorageKey::new(e.operator_id))
            .await
            .unwrap();
        assert!(
            op.current_total_stake > 0 || op.current_total_shares > 0,
            "operator should have stake or shares"
        );

        let _owner: SubxtAccountId32 = block_ext
            .read_storage(
                "Domains",
                "OperatorIdOwner",
                StaticStorageKey::new(e.operator_id),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_decode_operator_nominated() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = H256::from_str(OPERATOR_NOMINATED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = block_ext.events().await.unwrap();

        let nominated: Vec<_> = events
            .find::<OperatorNominated>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!nominated.is_empty(), "should find OperatorNominated event");
        let e = &nominated[0];
        assert!(e.operator_id > 0, "operator_id should be > 0");
        assert_ne!(
            e.nominator_id.0, [0u8; 32],
            "nominator_id should not be all zeros"
        );
    }

    #[tokio::test]
    async fn test_decode_withdrew_stake() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = H256::from_str(WITHDREW_STAKE_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = block_ext.events().await.unwrap();

        let withdrew: Vec<_> = events
            .find::<WithdrewStake>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!withdrew.is_empty(), "should find WithdrewStake event");
        let e = &withdrew[0];
        assert_ne!(
            e.nominator_id.0, [0u8; 32],
            "nominator_id should not be all zeros"
        );
    }

    #[tokio::test]
    async fn test_decode_domain_epoch_completed() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = H256::from_str(DOMAIN_EPOCH_COMPLETED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = block_ext.events().await.unwrap();

        let completed: Vec<_> = events
            .find::<DomainEpochCompleted>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !completed.is_empty(),
            "should find DomainEpochCompleted event"
        );
        let e = &completed[0];
        assert!(e.completed_epoch_index > 0, "epoch index should be > 0");
    }

    // ── End-to-end tests (RPC + pgtemp DB) ───────────────────────────

    #[tokio::test]
    async fn test_index_staking_operator_registered() {
        let test_db = get_db().await;
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = H256::from_str(OPERATOR_REGISTERED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let block_number = block_ext.number;

        index_staking_for_block(block_number, &test_db.db, &subspace)
            .await
            .unwrap();

        // Find what operator_id was registered in this block
        let events = block_ext.events().await.unwrap();
        let registered: Vec<_> = events
            .find::<OperatorRegistered>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!registered.is_empty());
        let operator_id = registered[0].operator_id;

        // Verify operator was inserted into DB
        let row = test_db.db.get_operator(operator_id as i64).await.unwrap();
        assert!(row.is_some(), "operator should exist in DB after indexing");
        let row = row.unwrap();
        assert_eq!(row.operator_id, operator_id.to_string());
        assert_eq!(row.status, "registered");
        assert!(
            !row.signing_key.is_empty(),
            "signing_key should be populated"
        );
        assert!(
            row.signing_key.starts_with("0x"),
            "signing_key should be hex"
        );
    }

    #[tokio::test]
    async fn test_index_staking_operator_nominated() {
        let test_db = get_db().await;
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        // First, decode the event to learn which operator_id is referenced
        let block_hash = H256::from_str(OPERATOR_NOMINATED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let block_number = block_ext.number;
        let events = block_ext.events().await.unwrap();
        let nominated: Vec<_> = events
            .find::<OperatorNominated>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!nominated.is_empty());
        let operator_id = nominated[0].operator_id;

        // Pre-insert the operator so the nominator upsert has a valid context
        test_db
            .db
            .upsert_operator(sample_operator(operator_id))
            .await
            .unwrap();

        // Run the indexer
        index_staking_for_block(block_number, &test_db.db, &subspace)
            .await
            .unwrap();

        // Verify nominator was inserted as active
        let count = test_db
            .db
            .get_active_nominator_count(operator_id as i64)
            .await
            .unwrap();
        assert!(
            count > 0,
            "at least one active nominator should exist after OperatorNominated"
        );
    }

    #[tokio::test]
    async fn test_index_staking_withdrew_stake() {
        let test_db = get_db().await;
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        // Decode the event to learn operator_id and nominator address
        let block_hash = H256::from_str(WITHDREW_STAKE_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let block_number = block_ext.number;
        let events = block_ext.events().await.unwrap();
        let withdrew: Vec<_> = events
            .find::<WithdrewStake>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!withdrew.is_empty());
        let operator_id = withdrew[0].operator_id;
        let nominator_address =
            sp_core::crypto::AccountId32::new(withdrew[0].nominator_id.0).to_string();

        // Pre-insert operator
        test_db
            .db
            .upsert_operator(sample_operator(operator_id))
            .await
            .unwrap();

        // Pre-insert nominator as "active" at an earlier block
        test_db
            .db
            .upsert_nominator(operator_id, &nominator_address, "active", 1)
            .await
            .unwrap();
        let count = test_db
            .db
            .get_active_nominator_count(operator_id as i64)
            .await
            .unwrap();
        assert!(count > 0, "nominator should be active before withdrawal");

        // Run the indexer for the WithdrewStake block
        index_staking_for_block(block_number, &test_db.db, &subspace)
            .await
            .unwrap();

        // Verify the specific nominator's status changed to withdrawn.
        // (The block may also contain OperatorNominated events that add other
        // active nominators for the same operator, so we query this nominator directly.)
        let status: (String,) = sqlx::query_as(
            "SELECT status FROM indexer.nominators WHERE operator_id = $1 AND address = $2",
        )
        .bind(operator_id as i64)
        .bind(&nominator_address)
        .fetch_one(&*test_db.db.pool)
        .await
        .unwrap();
        assert_eq!(
            status.0, "withdrawn",
            "nominator should be withdrawn after WithdrewStake event"
        );
    }

    #[tokio::test]
    async fn test_index_staking_domain_epoch_completed() {
        let test_db = get_db().await;
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = H256::from_str(DOMAIN_EPOCH_COMPLETED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let block_number = block_ext.number;

        // Decode the event to learn the domain_id
        let events = block_ext.events().await.unwrap();
        let completed: Vec<_> = events
            .find::<DomainEpochCompleted>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!completed.is_empty());
        let domain_id = completed[0].domain_id.clone();

        // Read the staking summary for this domain to get active operators
        let summary: crate::types::StakingSummary = block_ext
            .read_storage(
                "Domains",
                "DomainStakingSummary",
                StaticStorageKey::new(domain_id),
            )
            .await
            .unwrap();
        assert!(
            !summary.current_operators.is_empty(),
            "domain should have active operators"
        );

        // Pre-insert all active operators so update_operator_stats can update them
        for &op_id in summary.current_operators.keys() {
            test_db
                .db
                .upsert_operator(sample_operator(op_id))
                .await
                .unwrap();
        }

        // Run the indexer
        index_staking_for_block(block_number, &test_db.db, &subspace)
            .await
            .unwrap();

        // Verify share prices were stored for at least one operator
        let first_op_id = *summary.current_operators.keys().next().unwrap();
        let rows = test_db
            .db
            .get_share_prices_latest(first_op_id as i64, 10)
            .await
            .unwrap();
        assert!(
            !rows.is_empty(),
            "share prices should be stored after DomainEpochCompleted"
        );
    }
}
