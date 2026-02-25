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
///   withdrawals:                          BTreeMap<u32, { amount_to_unlock: u128, storage_fee_refund: u128 }>
///   withdrawal_in_shares:                 Option<{
///       domain_epoch: (DomainId(u32), EpochIndex(u32)),
///       unlock_at_confirmed_domain_block_number: u32,
///       shares: u128,
///       storage_fee_refund: u128,
///   }>
///   storage_fee_refund:                   u128   (top-level cumulative)
///
/// When `withdrawal_in_shares` is present we extract shares and its refund directly.
/// When it is `None` (epoch transition already converted shares to balance in the same
/// block), we fall back to the last entry in the `withdrawals` BTreeMap for the
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

        // withdrawals: BTreeMap<DomainBlockNumber, WithdrawalInBalance>
        // Track the last entry (highest block number) as fallback.
        let len = parity_scale_codec::Compact::<u32>::decode(input)?.0;
        let mut last_amount: u128 = 0;
        let mut last_refund: u128 = 0;
        for _ in 0..len {
            let _ = u32::decode(input)?; // DomainBlockNumber key
            last_amount = u128::decode(input)?; // amount_to_unlock
            last_refund = u128::decode(input)?; // storage_fee_refund
        }

        // withdrawal_in_shares: Option<WithdrawalInShares>
        let variant = u8::decode(input)?;
        if variant == 0 {
            // Shares already epoch-converted; fall back to the last BTreeMap entry.
            let _ = u128::decode(input)?; // top-level storage_fee_refund (cumulative, skip)
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
        let _ = u128::decode(input)?; // top-level storage_fee_refund (cumulative, skip)
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
