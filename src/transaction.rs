//! Minimal record of a processed transaction, kept to service disputes.

use rust_decimal::Decimal;

use crate::operation::ClientId;

/// A processed transaction's client and amount, stored by transaction id, along
/// with whether it is currently under dispute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTx {
    pub client: ClientId,
    pub amount: Decimal,
    pub disputed: bool,
}
