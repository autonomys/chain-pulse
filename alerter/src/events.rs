//! Module to monitor AI3 transfers and other events

use crate::error::Error;
use crate::event_types::{
    BalanceDeposit, BalanceTransfer, BalanceWithdraw, CodeUpdated, DomainInstantiated,
    DomainRuntimeUpgraded, Event, FraudProofProcessed, LowBalanceEvent, OperatorOffline,
    OperatorSlashed, Sudo, TransferDirection, TransferEvent, TransferKnownAccountEvent,
};
use crate::slack::{Alert, AlertSink};
use crate::{Account, BalanceAlert};
use log::{debug, error, info, warn};
use parity_scale_codec::Decode;
use shared::subspace::{AccountId, Balance, BlockExt, BlocksStream};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use subxt::events::Events;
use subxt::storage::StaticStorageKey;
use subxt_core::config::SubstrateConfig;
use subxt_core::events::StaticEvent;

struct ResolvedBalanceAlert {
    name: String,
    address: String,
    threshold: Balance,
}

/// Minimal decode target for `System.Account` storage.
/// Layout matches `frame_system::AccountInfo<u32, pallet_balances::AccountData<u128>>`.
#[derive(Decode)]
struct StorageAccountData {
    free: Balance,
    _reserved: Balance,
    _frozen: Balance,
    _flags: u128,
}

#[derive(Decode)]
struct StorageAccountInfo {
    _nonce: u32,
    _consumers: u32,
    _providers: u32,
    _sufficients: u32,
    data: StorageAccountData,
}

pub(crate) async fn watch_events(
    mut stream: BlocksStream,
    alert_sink: AlertSink,
    accounts: Vec<Account>,
    balance_alerts: Vec<BalanceAlert>,
    token_decimals: u8,
) -> Result<(), Error> {
    info!("Watching block events...");
    let transfer_account_map = account_mapped_name(accounts);
    let balance_alert_map = build_balance_alert_map(balance_alerts, token_decimals);
    loop {
        let blocks_ext = stream.recv().await?;
        for block in blocks_ext.blocks {
            let block_events = block.events().await?;
            let mut events: Vec<Event> = vec![];

            let mut transfers = filter_known_account_transfers(
                block_events
                    .find::<BalanceTransfer>()
                    .try_collect::<Vec<_>>()?,
                &transfer_account_map,
            );
            transfers.extend(filter_known_account_transfers(
                block_events
                    .find::<BalanceDeposit>()
                    .try_collect::<Vec<_>>()?,
                &transfer_account_map,
            ));

            let withdrawals = block_events
                .find::<BalanceWithdraw>()
                .try_collect::<Vec<_>>()?;
            let low_balance_events =
                check_low_balances(&block, &withdrawals, &balance_alert_map).await;
            transfers.extend(filter_known_account_transfers(
                withdrawals,
                &transfer_account_map,
            ));

            events.extend(transfers.into_iter().map(Into::into).collect::<Vec<_>>());
            events.extend(low_balance_events.into_iter().map(Into::into));
            events.extend(as_events::<DomainRuntimeUpgraded>(&block_events)?);
            events.extend(as_events::<DomainInstantiated>(&block_events)?);
            events.extend(as_events::<FraudProofProcessed>(&block_events)?);
            events.extend(as_events::<OperatorSlashed>(&block_events)?);
            events.extend(as_events::<OperatorOffline>(&block_events)?);
            events.extend(as_events::<Sudo>(&block_events)?);
            events.extend(as_events::<CodeUpdated>(&block_events)?);
            debug!(
                "Found {} events in block {}[{}]",
                events.len(),
                block.number,
                block.hash
            );
            events.into_iter().for_each(|event| {
                if let Err(err) = alert_sink.send(Alert::Event(event)) {
                    error!("⛔️failed to send block event alert: {err}");
                }
            })
        }
    }
}

/// Builds a lookup map for balance-alert accounts with thresholds converted to
/// Shannons using the network's token decimals.
fn build_balance_alert_map(
    alerts: Vec<BalanceAlert>,
    token_decimals: u8,
) -> BTreeMap<AccountId, ResolvedBalanceAlert> {
    let scale = 10u128.pow(token_decimals as u32);
    alerts
        .into_iter()
        .map(|alert| {
            let account_id =
                AccountId::from_str(&alert.address).expect("Must be a valid SS58 address");
            let threshold = alert.threshold_ai3 as Balance * scale;
            (
                account_id,
                ResolvedBalanceAlert {
                    name: alert.name,
                    address: alert.address,
                    threshold,
                },
            )
        })
        .collect()
}

/// For each `account_balance_alerts` account that submitted an extrinsic in this block
/// (detected via `Balances::Withdraw` fee deduction), queries its free balance and
/// emits a `LowBalanceEvent` if it has dropped below the configured threshold.
/// No RPC call is made on blocks where the account has no activity.
async fn check_low_balances(
    block: &BlockExt,
    withdrawals: &[BalanceWithdraw],
    balance_alerts: &BTreeMap<AccountId, ResolvedBalanceAlert>,
) -> Vec<LowBalanceEvent> {
    let mut alerts = vec![];
    let mut checked: BTreeSet<AccountId> = BTreeSet::new();

    for withdrawal in withdrawals {
        let Some(account_id) = withdrawal.from() else {
            continue;
        };
        if !balance_alerts.contains_key(&account_id) {
            continue;
        }
        if !checked.insert(account_id.clone()) {
            continue;
        }

        let ResolvedBalanceAlert {
            name,
            address,
            threshold,
        } = balance_alerts.get(&account_id).expect("checked above; qed");

        let key = StaticStorageKey::new(account_id.clone());
        match block
            .try_read_storage::<_, StorageAccountInfo>("System", "Account", key)
            .await
        {
            Ok(Some(info)) => {
                if info.data.free < *threshold {
                    alerts.push(LowBalanceEvent {
                        name: name.clone(),
                        address: address.clone(),
                        balance: info.data.free,
                        threshold: *threshold,
                    });
                }
            }
            Ok(None) => {
                warn!("System.Account storage missing for {address} — balance check skipped");
            }
            Err(err) => {
                warn!(
                    "Failed to read balance for {address} in block {}: {err}",
                    block.number
                );
            }
        }
    }

    alerts
}

fn as_events<E: StaticEvent + Into<Event>>(
    block_events: &Events<SubstrateConfig>,
) -> Result<Vec<Event>, Error> {
    Ok(block_events
        .find::<E>()
        .try_collect::<Vec<_>>()?
        .into_iter()
        .map(Into::into)
        .collect())
}

fn account_mapped_name(accounts: Vec<Account>) -> BTreeMap<AccountId, Account> {
    accounts
        .into_iter()
        .map(|account| {
            (
                AccountId::from_str(&account.address).expect("Must be a valid SS58 address"),
                account,
            )
        })
        .collect()
}

fn filter_known_account_transfers<T: TransferEvent>(
    events: Vec<T>,
    accounts: &BTreeMap<AccountId, Account>,
) -> Vec<TransferKnownAccountEvent> {
    events
        .into_iter()
        .filter_map(|event| {
            let maybe_from = event.from();
            let maybe_to = event.to();
            let amount = event.amount();
            if let Some(from) = maybe_from
                && accounts.contains_key(&from)
            {
                let account = accounts.get(&from).expect("checked above; qed").clone();
                Some(TransferKnownAccountEvent {
                    direction: TransferDirection::Sender,
                    transfer_type: event.transfer_type(),
                    name: account.name,
                    address: account.address,
                    amount,
                })
            } else if let Some(to) = maybe_to
                && accounts.contains_key(&to)
            {
                let account = accounts.get(&to).expect("checked above; qed").clone();
                Some(TransferKnownAccountEvent {
                    direction: TransferDirection::Receiver,
                    transfer_type: event.transfer_type(),
                    name: account.name,
                    address: account.address,
                    amount,
                })
            } else {
                None
            }
        })
        .collect()
}
