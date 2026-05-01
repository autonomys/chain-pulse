//! Decode types for live RPC storage reads (operator data only).
//!
//! These mirror the on-chain SCALE layout of pallet-domains.
//! Manual `impl Decode` is used for types with skip-fields to avoid storing unused data.

use parity_scale_codec::Decode;

/// DomainEpoch(DomainId(u32), EpochIndex(u32)) — helper for manual decode only.
/// Read two u32s and discard for the Deregistered variant.
struct DomainEpochHelper;

impl DomainEpochHelper {
    fn skip<I: parity_scale_codec::Input>(input: &mut I) -> Result<(), parity_scale_codec::Error> {
        let _ = u32::decode(input)?; // domain_id
        let _ = u32::decode(input)?; // epoch_index
        Ok(())
    }
}

pub(crate) enum OperatorStatusCompact {
    Registered,
    Deregistered,
    Slashed,
    PendingSlash,
    InvalidBundle,
    Deactivated,
}

impl OperatorStatusCompact {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Deregistered => "deregistered",
            Self::Slashed => "slashed",
            Self::PendingSlash => "pending_slash",
            Self::InvalidBundle => "invalid_bundle",
            Self::Deactivated => "deactivated",
        }
    }
}

impl Decode for OperatorStatusCompact {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        let variant = u8::decode(input)?;
        match variant {
            0 => Ok(Self::Registered),
            1 => {
                // Deregistered { domain_epoch: DomainEpoch, unlock_at_block: u32 }
                DomainEpochHelper::skip(input)?;
                let _ = u32::decode(input)?; // unlock_at_block
                Ok(Self::Deregistered)
            }
            2 => Ok(Self::Slashed),
            3 => Ok(Self::PendingSlash),
            4 => {
                let _ = <[u8; 32] as Decode>::decode(input)?; // bad_receipt_hash
                Ok(Self::InvalidBundle)
            }
            5 => {
                let _ = u32::decode(input)?; // at_epoch_index
                Ok(Self::Deactivated)
            }
            n => Err(
                parity_scale_codec::Error::from("Unknown OperatorStatus variant")
                    .chain(format!("variant index: {n}")),
            ),
        }
    }
}

/// On-chain SCALE layout for `Domains::Withdrawals` storage double-map.
///
///   total_withdrawal_amount:              u128
///   total_storage_fee_withdrawal:         u128
///   withdrawals:                          VecDeque<{
///       unlock_at_confirmed_domain_block_number: u32,
///       amount_to_unlock: u128,
///       storage_fee_refund: u128,
///   }>
///   withdrawal_in_shares:                 Option<{
///       domain_epoch: (DomainId(u32), EpochIndex(u32)),
///       unlock_at_confirmed_domain_block_number: u32,
///       shares: u128,
///       storage_fee_refund: u128,
///   }>
///
/// When `withdrawal_in_shares` is present we extract shares and its refund directly.
/// When it is `None` (epoch transition already converted shares to balance in the same
/// block), we fall back to the last entry in the `withdrawals` VecDeque for the
/// converted balance amount and per-withdrawal storage_fee_refund.
pub(crate) struct NominatorWithdrawal {
    pub(crate) shares: u128,
    pub(crate) amount: u128,
    pub(crate) storage_fee_refund: u128,
}

impl Decode for NominatorWithdrawal {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        let _ = u128::decode(input)?; // total_withdrawal_amount
        let _ = u128::decode(input)?; // total_storage_fee_withdrawal

        // withdrawals: VecDeque<WithdrawalInBalance>
        // Track the last entry as fallback when shares have already been converted.
        let len = parity_scale_codec::Compact::<u32>::decode(input)?.0;
        let mut last_amount: u128 = 0;
        let mut last_refund: u128 = 0;
        for _ in 0..len {
            let _ = u32::decode(input)?; // unlock_at_confirmed_domain_block_number
            last_amount = u128::decode(input)?; // amount_to_unlock
            last_refund = u128::decode(input)?; // storage_fee_refund
        }

        // withdrawal_in_shares: Option<WithdrawalInShares>
        let variant = u8::decode(input)?;
        if variant == 0 {
            // Shares already epoch-converted; fall back to the last VecDeque entry.
            return Ok(Self {
                shares: 0,
                amount: last_amount,
                storage_fee_refund: last_refund,
            });
        }
        let _ = u32::decode(input)?; // domain_epoch.domain_id
        let _ = u32::decode(input)?; // domain_epoch.epoch_index
        let _ = u32::decode(input)?; // unlock_at_confirmed_domain_block_number
        let shares = u128::decode(input)?;
        let storage_fee_refund = u128::decode(input)?;
        Ok(Self {
            shares,
            amount: 0,
            storage_fee_refund,
        })
    }
}

/// On-chain SCALE layout (Autonomys mainnet):
///   signing_key:                [u8; 32]
///   current_domain_id:          u32
///   next_domain_id:             u32   (skipped)
///   minimum_nominator_stake:    u128
///   nomination_tax:             u8
///   current_total_stake:        u128
///   current_total_shares:       u128
///   status:                     OperatorStatus (enum)
///   deposits_in_epoch:          u128  (skipped)
///   withdrawals_in_epoch:       u128  (skipped)
///   total_storage_fee_deposit:  u128
pub(crate) struct FullOperator {
    pub(crate) signing_key: [u8; 32],
    pub(crate) current_domain_id: u32,
    pub(crate) minimum_nominator_stake: u128,
    pub(crate) nomination_tax: u8,
    pub(crate) current_total_stake: u128,
    pub(crate) current_total_shares: u128,
    pub(crate) status: OperatorStatusCompact,
    pub(crate) total_storage_fee_deposit: u128,
}

impl Decode for FullOperator {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        let signing_key = <[u8; 32] as Decode>::decode(input)?;
        let current_domain_id = u32::decode(input)?;
        let _ = u32::decode(input)?; // next_domain_id
        let minimum_nominator_stake = u128::decode(input)?;
        let nomination_tax = u8::decode(input)?;
        let current_total_stake = u128::decode(input)?;
        let current_total_shares = u128::decode(input)?;
        let status = OperatorStatusCompact::decode(input)?;
        let _ = u128::decode(input)?; // deposits_in_epoch
        let _ = u128::decode(input)?; // withdrawals_in_epoch
        let total_storage_fee_deposit = u128::decode(input)?;
        Ok(Self {
            signing_key,
            current_domain_id,
            minimum_nominator_stake,
            nomination_tax,
            current_total_stake,
            current_total_shares,
            status,
            total_storage_fee_deposit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{OperatorRegistered, WithdrewStake};
    use shared::subspace::Subspace;
    use std::str::FromStr;
    use subxt::storage::StaticStorageKey;

    const RPC_URL: &str = "wss://rpc.mainnet.autonomys.xyz/ws";

    /// Block containing a WithdrewStake event (post runtime upgrade layout).
    const WITHDREW_STAKE_BLOCK: u32 = 6_718_423;

    /// Block containing an OperatorRegistered event (used by staking.rs tests too).
    const OPERATOR_REGISTERED_HASH: &str =
        "0x9749ce3959c6e613a85f1576331ebb138aa3f00492dcde3b37ae978bb399c364";

    #[tokio::test]
    async fn test_decode_nominator_withdrawal() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_ext = subspace
            .block_ext_at_number(WITHDREW_STAKE_BLOCK)
            .await
            .unwrap();
        let events = block_ext.events().await.unwrap();

        let withdrew: Vec<_> = events
            .find::<WithdrewStake>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !withdrew.is_empty(),
            "block {WITHDREW_STAKE_BLOCK} should contain a WithdrewStake event"
        );

        for e in &withdrew {
            let withdrawal: NominatorWithdrawal = block_ext
                .try_read_storage(
                    "Domains",
                    "Withdrawals",
                    (
                        StaticStorageKey::new(e.operator_id),
                        StaticStorageKey::new(e.nominator_id.clone()),
                    ),
                )
                .await
                .expect("Withdrawals storage should decode without error")
                .expect("Withdrawals entry should exist after WithdrewStake");

            // At least one of shares or amount must be non-zero.
            assert!(
                withdrawal.shares > 0 || withdrawal.amount > 0,
                "withdrawal for operator {} should have shares or amount",
                e.operator_id,
            );
        }
    }

    #[tokio::test]
    async fn test_decode_full_operator() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        let block_hash = subxt::utils::H256::from_str(OPERATOR_REGISTERED_HASH).unwrap();
        let block_ext = subspace.block_ext_at_hash(block_hash).await.unwrap();
        let events = block_ext.events().await.unwrap();

        let registered: Vec<_> = events
            .find::<OperatorRegistered>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!registered.is_empty());

        let e = &registered[0];
        let op: FullOperator = block_ext
            .read_storage("Domains", "Operators", StaticStorageKey::new(e.operator_id))
            .await
            .expect("FullOperator should decode without error");

        assert!(
            op.current_total_stake > 0 || op.current_total_shares > 0,
            "operator should have stake or shares"
        );
        assert_eq!(
            op.status.as_str(),
            "registered",
            "freshly registered operator should have 'registered' status"
        );
        assert!(
            op.minimum_nominator_stake > 0,
            "minimum_nominator_stake should be > 0"
        );
    }

    #[tokio::test]
    async fn test_decode_full_operator_at_recent_block() {
        let subspace = Subspace::new_from_url(RPC_URL)
            .await
            .unwrap()
            .block_provider();

        // Use the same block as WithdrewStake — this is post-upgrade so it
        // validates the FullOperator layout hasn't drifted either.
        let block_ext = subspace
            .block_ext_at_number(WITHDREW_STAKE_BLOCK)
            .await
            .unwrap();

        let withdrew: Vec<_> = block_ext
            .events()
            .await
            .unwrap()
            .find::<WithdrewStake>()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!withdrew.is_empty());

        let op: FullOperator = block_ext
            .read_storage(
                "Domains",
                "Operators",
                StaticStorageKey::new(withdrew[0].operator_id),
            )
            .await
            .expect("FullOperator should decode at recent block without error");

        // Sanity check: the operator referenced by a withdrawal should exist.
        assert!(
            !op.signing_key.iter().all(|&b| b == 0),
            "signing_key should not be all zeros"
        );
    }
}
