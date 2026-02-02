// TODO:
#![allow(dead_code)]

use parity_scale_codec::Decode;
use scale_decode::DecodeAsType;
use scale_decode::ext::primitive_types::U256;
use scale_encode::EncodeAsType;
use shared::subspace::Balance;
use subxt::events::StaticEvent;
use subxt::utils::{AccountId32, H160};

#[derive(Debug, Copy, Clone, Decode, DecodeAsType, EncodeAsType, Eq, PartialEq)]
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

/// Unique identifier of a domain.
#[derive(Debug, Clone, DecodeAsType, Decode, EncodeAsType, Eq, PartialEq)]
pub(crate) struct DomainId(pub(crate) u32);

#[derive(Debug, Clone, DecodeAsType, Decode, EncodeAsType, Eq, PartialEq)]
pub(crate) enum ChainId {
    Consensus,
    Domain(DomainId),
}

pub(crate) type XdmChannelId = U256Compat;
pub(crate) type XdmNonce = U256Compat;
pub(crate) type XdmMessageId = (XdmChannelId, XdmNonce);

#[derive(Debug, Clone, DecodeAsType, Eq, PartialEq)]
pub(crate) struct OutgoingTransferInitiated {
    pub(crate) chain_id: ChainId,
    pub(crate) message_id: XdmMessageId,
    pub(crate) amount: Balance,
}

impl StaticEvent for OutgoingTransferInitiated {
    const PALLET: &'static str = "Transporter";
    const EVENT: &'static str = "OutgoingTransferInitiated";
}

#[derive(Debug, Clone, DecodeAsType, Eq, PartialEq)]
pub(crate) struct OutgoingTransferFailed {
    pub(crate) chain_id: ChainId,
    pub(crate) message_id: XdmMessageId,
    // TODO: capture error as well
}

impl StaticEvent for OutgoingTransferFailed {
    const PALLET: &'static str = "Transporter";
    const EVENT: &'static str = "OutgoingTransferFailed";
}

impl From<OutgoingTransferFailed> for Event {
    fn from(value: OutgoingTransferFailed) -> Self {
        Event::OutgoingTransferFailed(value)
    }
}

#[derive(Debug, Clone, DecodeAsType, Eq, PartialEq)]
pub(crate) struct OutgoingTransferSuccessful {
    pub(crate) chain_id: ChainId,
    pub(crate) message_id: XdmMessageId,
}

impl StaticEvent for OutgoingTransferSuccessful {
    const PALLET: &'static str = "Transporter";
    const EVENT: &'static str = "OutgoingTransferSuccessful";
}

impl From<OutgoingTransferSuccessful> for Event {
    fn from(value: OutgoingTransferSuccessful) -> Self {
        Event::OutgoingTransferSuccessful(value)
    }
}

#[derive(Debug, Clone, DecodeAsType, Eq, PartialEq)]
pub(crate) struct IncomingTransferSuccessful {
    pub(crate) chain_id: ChainId,
    pub(crate) message_id: XdmMessageId,
    pub(crate) amount: Balance,
}

impl StaticEvent for IncomingTransferSuccessful {
    const PALLET: &'static str = "Transporter";
    const EVENT: &'static str = "IncomingTransferSuccessful";
}

impl From<IncomingTransferSuccessful> for Event {
    fn from(value: IncomingTransferSuccessful) -> Self {
        Event::IncomingTransferSuccessful(value)
    }
}

#[derive(Debug, Clone, DecodeAsType, Decode, Eq, PartialEq)]
pub(crate) enum MultiAccountId {
    /// 32 byte account Id.
    AccountId32(AccountId32),
    /// 20 byte account Id. Ex: Ethereum
    AccountId20(H160),
    /// Some raw bytes
    Raw(Vec<u8>),
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

#[derive(Debug, Clone, DecodeAsType, Eq, PartialEq)]
pub(crate) struct OutgoingTransferInitiatedWithTransfer {
    pub(crate) message_id: XdmMessageId,
    pub(crate) transfer: Transfer,
}

impl From<OutgoingTransferInitiatedWithTransfer> for Event {
    fn from(value: OutgoingTransferInitiatedWithTransfer) -> Self {
        Event::OutgoingTransferInitiated(value)
    }
}

/// Overarching event type
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Event {
    OutgoingTransferInitiated(OutgoingTransferInitiatedWithTransfer),
    OutgoingTransferFailed(OutgoingTransferFailed),
    OutgoingTransferSuccessful(OutgoingTransferSuccessful),
    IncomingTransferSuccessful(IncomingTransferSuccessful),
}
