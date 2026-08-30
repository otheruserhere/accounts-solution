//! Minimal record of a processed transaction, kept to service disputes.

use rust_decimal::Decimal;

use crate::operation::ClientId;

/// Where a transaction sits in the dispute lifecycle. `ChargedBack` is terminal,
/// so a reversed transaction can never be disputed or charged back again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisputeState {
    Undisputed,
    Disputed,
    ChargedBack,
}

/// A processed transaction's client and amount, stored by transaction id, along
/// with its dispute lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTx {
    pub client: ClientId,
    pub amount: Decimal,
    pub state: DisputeState,
}
