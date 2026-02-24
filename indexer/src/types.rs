use parity_scale_codec::Decode;
use scale_decode::DecodeAsType;
use scale_decode::ext::primitive_types::U256;
use scale_encode::EncodeAsType;
use serde::{Deserialize, Serialize};
use shared::subspace::Balance;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgTypeInfo, PgValueRef};
use sqlx::{Decode as SqlxDecode, Postgres, Type};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use subxt::utils::{AccountId32, H160, to_hex};

#[derive(Serialize, Deserialize, Default, PartialEq, Debug, Copy, Clone)]
pub(crate) struct U128Compat(pub(crate) u128);

impl<'r> SqlxDecode<'r, Postgres> for U128Compat {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let bd = <String as SqlxDecode<Postgres>>::decode(value)?;
        let decoded = bd.parse::<u128>()?;
        Ok(Self(decoded))
    }
}

impl Type<Postgres> for U128Compat {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("NUMERIC")
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        ty.type_eq(&PgTypeInfo::with_name("TEXT"))
    }
}

#[derive(
    Serialize, Deserialize, Debug, Copy, Clone, Decode, DecodeAsType, EncodeAsType, Eq, PartialEq,
)]
pub(crate) struct U256Compat([u64; 4]);

impl From<U256> for U256Compat {
    fn from(value: U256) -> Self {
        Self(value.0)
    }
}

impl From<U256Compat> for U256 {
    fn from(value: U256Compat) -> Self {
        U256(value.0)
    }
}

impl Display for U256Compat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value: U256 = (*self).into();
        write!(f, "{value}")
    }
}

/// Unique identifier of a domain.
#[derive(Debug, Clone, DecodeAsType, Decode, EncodeAsType, Eq, PartialEq)]
pub(crate) struct DomainId(pub(crate) u32);

impl Display for DomainId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, DecodeAsType, Decode, EncodeAsType, Eq, PartialEq)]
pub(crate) enum ChainId {
    Consensus,
    Domain(DomainId),
}

impl Display for ChainId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainId::Consensus => write!(f, "Consensus"),
            ChainId::Domain(domain_id) => write!(f, "Domain({domain_id})"),
        }
    }
}

pub(crate) type XdmChannelId = U256Compat;
pub(crate) type XdmNonce = U256Compat;
pub(crate) type XdmMessageId = (XdmChannelId, XdmNonce);

#[derive(Debug, Clone, DecodeAsType, Decode, Eq, PartialEq)]
pub(crate) enum MultiAccountId {
    /// 32 byte account Id.
    AccountId32(AccountId32),
    /// 20 byte account Id. Ex: Ethereum
    AccountId20(H160),
    /// Some raw bytes
    Raw(Vec<u8>),
}

impl Display for MultiAccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            MultiAccountId::AccountId32(data) => {
                let acc = sp_core::crypto::AccountId32::new(data.0);
                acc.to_string()
            }
            MultiAccountId::AccountId20(data) => to_hex(data.0),
            MultiAccountId::Raw(data) => to_hex(data),
        };

        write!(f, "{str}")
    }
}

#[derive(Debug, Clone, DecodeAsType, Decode, Eq, PartialEq)]
pub(crate) struct Location {
    /// Unique identity of chain.
    pub chain_id: ChainId,
    /// Unique account on chain.
    pub account_id: MultiAccountId,
}

#[derive(Debug, Clone, DecodeAsType, Decode, Eq, PartialEq)]
pub(crate) struct Transfer {
    /// Amount being transferred between entities.
    pub amount: Balance,
    /// Sender location of the transfer.
    pub sender: Location,
    /// Receiver location of the transfer.
    pub receiver: Location,
}

/// Mirrors `pallet_domains::staking::DomainEpoch(DomainId, EpochIndex)`.
/// Used as the second key of the `OperatorEpochSharePrice` storage double-map.
/// Both EncodeAsType and DecodeAsType are required by StaticStorageKey<T>.
#[derive(Debug, Clone, DecodeAsType, EncodeAsType, Eq, PartialEq)]
pub(crate) struct DomainEpoch(pub(crate) DomainId, pub(crate) u32);

/// Partial decode of `pallet_domains::staking::StakingSummary`.
/// Only `current_operators` is needed; the preceding fields are read-and-discarded
/// and the remaining fields (`next_operators`, `current_epoch_rewards`) are left in
/// the input — SCALE Decode does not require consuming all bytes.
#[derive(Debug)]
pub(crate) struct StakingSummary {
    pub(crate) current_operators: BTreeMap<u64, u128>,
}

impl Decode for StakingSummary {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        let _ = u32::decode(input)?; // current_epoch_index
        let _ = u128::decode(input)?; // current_total_stake
        let current_operators = BTreeMap::<u64, u128>::decode(input)?;
        Ok(Self { current_operators })
    }
}
