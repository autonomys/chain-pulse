//! Event types that implement `subxt::events::StaticEvent`.
//!
//! XDM events (Transporter pallet) and staking events (Domains pallet).

use crate::types::{ChainId, DomainId, Transfer, XdmMessageId};
use scale_decode::DecodeAsType;
use shared::subspace::Balance;
use subxt::events::StaticEvent;
use subxt::utils::AccountId32;

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

/// Emitted by pallet `Domains` after each epoch finalises share prices.
#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct DomainEpochCompleted {
    pub(crate) domain_id: DomainId,
    /// EpochIndex is a type alias for u32 on-chain.
    pub(crate) completed_epoch_index: u32,
}

impl StaticEvent for DomainEpochCompleted {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "DomainEpochCompleted";
}

#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct OperatorNominated {
    pub(crate) operator_id: u64,
    pub(crate) nominator_id: AccountId32,
    // `amount` is present in the on-chain event but not needed here;
    // DecodeAsType decodes by field-name matching so omitting it is safe.
}

impl StaticEvent for OperatorNominated {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "OperatorNominated";
}

#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct WithdrewStake {
    pub(crate) operator_id: u64,
    pub(crate) nominator_id: AccountId32,
}

impl StaticEvent for WithdrewStake {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "WithdrewStake";
}

#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct NominatorUnlocked {
    pub(crate) operator_id: u64,
    pub(crate) nominator_id: AccountId32,
}

impl StaticEvent for NominatorUnlocked {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "NominatorUnlocked";
}

#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct OperatorRegistered {
    pub(crate) operator_id: u64,
}

impl StaticEvent for OperatorRegistered {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "OperatorRegistered";
}

#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct OperatorDeregistered {
    pub(crate) operator_id: u64,
}

impl StaticEvent for OperatorDeregistered {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "OperatorDeregistered";
}

/// `reason` field is omitted — DecodeAsType matches by field name and skips unknowns.
#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct OperatorSlashed {
    pub(crate) operator_id: u64,
}

impl StaticEvent for OperatorSlashed {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "OperatorSlashed";
}

/// `reactivation_delay` is omitted — we only need operator_id.
#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct OperatorDeactivated {
    pub(crate) operator_id: u64,
}

impl StaticEvent for OperatorDeactivated {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "OperatorDeactivated";
}

#[derive(Debug, Clone, DecodeAsType)]
pub(crate) struct OperatorReactivated {
    pub(crate) operator_id: u64,
}

impl StaticEvent for OperatorReactivated {
    const PALLET: &'static str = "Domains";
    const EVENT: &'static str = "OperatorReactivated";
}
