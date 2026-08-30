//! Parsing of a CSV transaction row into a typed [`Operation`].

use eyre::{Result, bail, eyre};
use rust_decimal::Decimal;
use serde::Deserialize;

pub type ClientId = u16;
pub type TxId = u32;

/// The `type` column of an input row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RecordType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

/// A raw CSV row as deserialized by serde, before per-type validation.
#[derive(Debug, Clone, Deserialize)]
pub struct Record {
    #[serde(rename = "type")]
    kind: RecordType,
    client: ClientId,
    tx: TxId,
    // `default` also tolerates a row that omits the amount column entirely.
    #[serde(default)]
    amount: Option<Decimal>,
}

/// A single transaction read from the input CSV.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Credit to a client's account: increases available and total funds.
    Deposit {
        client: ClientId,
        tx: TxId,
        amount: Decimal,
    },
    /// Debit from a client's account: decreases available and total funds.
    Withdrawal {
        client: ClientId,
        tx: TxId,
        amount: Decimal,
    },
    /// Claim that a referenced transaction was erroneous; holds its funds.
    Dispute { client: ClientId, tx: TxId },
    /// Resolution of a dispute; releases the held funds.
    Resolve { client: ClientId, tx: TxId },
    /// Final reversal of a disputed transaction; withdraws held funds and locks.
    Chargeback { client: ClientId, tx: TxId },
}

impl TryFrom<Record> for Operation {
    type Error = eyre::Report;

    fn try_from(record: Record) -> Result<Self> {
        let Record {
            kind,
            client,
            tx,
            amount,
        } = record;

        match kind {
            RecordType::Deposit => Ok(Operation::Deposit {
                client,
                tx,
                amount: require_amount(amount)?,
            }),
            RecordType::Withdrawal => Ok(Operation::Withdrawal {
                client,
                tx,
                amount: require_amount(amount)?,
            }),
            RecordType::Dispute => Ok(Operation::Dispute { client, tx }),
            RecordType::Resolve => Ok(Operation::Resolve { client, tx }),
            RecordType::Chargeback => Ok(Operation::Chargeback { client, tx }),
        }
    }
}

impl Operation {
    /// Parse a single CSV row into an [`Operation`], tolerating surrounding
    /// whitespace and an empty or absent amount column.
    pub fn parse(line: &str) -> Result<Operation> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(line.as_bytes());

        let record: Record = reader
            .deserialize()
            .next()
            .ok_or_else(|| eyre!("no record found in line: {line:?}"))??;

        Operation::try_from(record)
    }
}

/// Validate that a deposit/withdrawal amount is present and non-negative.
fn require_amount(amount: Option<Decimal>) -> Result<Decimal> {
    let amount = amount.ok_or_else(|| eyre!("missing amount"))?;
    if amount.is_sign_negative() {
        bail!("amount must not be negative: {amount}");
    }
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn parses_deposit() {
        assert_eq!(
            Operation::parse("deposit, 1, 1, 1.0").unwrap(),
            Operation::Deposit {
                client: 1,
                tx: 1,
                amount: dec("1.0"),
            }
        );
    }

    #[test]
    fn parses_withdrawal() {
        assert_eq!(
            Operation::parse("withdrawal, 2, 5, 3.0").unwrap(),
            Operation::Withdrawal {
                client: 2,
                tx: 5,
                amount: dec("3.0"),
            }
        );
    }

    #[test]
    fn parses_four_decimal_precision() {
        assert_eq!(
            Operation::parse("deposit, 1, 6, 5.1234").unwrap(),
            Operation::Deposit {
                client: 1,
                tx: 6,
                amount: dec("5.1234"),
            }
        );
    }

    #[test]
    fn parses_dispute_resolve_chargeback_without_amount() {
        assert_eq!(
            Operation::parse("dispute, 1, 1,").unwrap(),
            Operation::Dispute { client: 1, tx: 1 }
        );
        assert_eq!(
            Operation::parse("resolve, 1, 1").unwrap(),
            Operation::Resolve { client: 1, tx: 1 }
        );
        assert_eq!(
            Operation::parse("chargeback, 2, 2,").unwrap(),
            Operation::Chargeback { client: 2, tx: 2 }
        );
    }

    #[test]
    fn errors_on_unknown_type() {
        assert!(Operation::parse("teleport, 1, 1, 1.0").is_err());
    }

    #[test]
    fn errors_on_missing_amount_for_deposit() {
        assert!(Operation::parse("deposit, 1, 1,").is_err());
    }

    #[test]
    fn errors_on_non_numeric_amount() {
        assert!(Operation::parse("deposit, 1, 1, abc").is_err());
    }

    #[test]
    fn errors_on_negative_amount() {
        assert!(Operation::parse("deposit, 1, 1, -1.0").is_err());
    }

    #[test]
    fn errors_on_invalid_client() {
        assert!(Operation::parse("deposit, 99999999, 1, 1.0").is_err());
    }
}
