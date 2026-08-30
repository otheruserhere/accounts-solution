//! Parsing of a CSV transaction row into a typed [`Operation`].

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use eyre::{Result, bail, eyre};
use iddqd::IdOrdMap;
use iddqd::id_ord_map::RefMut;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::account::Account;
use crate::transaction::StoredTx;

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
    /// Apply this operation to the account map, using the transaction store to
    /// record deposits/withdrawals and to service disputes.
    pub fn process(&self, accounts: &mut IdOrdMap<Account>, txs: &mut HashMap<TxId, StoredTx>) {
        match self {
            Operation::Deposit { client, tx, amount } => {
                account_mut(accounts, *client).deposit(*amount);
                record_tx(txs, *client, *tx, *amount);
                log::debug!("deposit tx {tx}: client {client} credited {amount}");
            }
            Operation::Withdrawal { client, tx, amount } => {
                match account_mut(accounts, *client).withdraw(*amount) {
                    Ok(()) => {
                        record_tx(txs, *client, *tx, *amount);
                        log::debug!("withdrawal tx {tx}: client {client} debited {amount}");
                    }
                    Err(err) => {
                        log::warn!("withdrawal tx {tx} for client {client} failed: {err}");
                    }
                }
            }
            Operation::Dispute { client, tx } => dispute(accounts, txs, *client, *tx),
            Operation::Resolve { client, tx } => resolve(accounts, txs, *client, *tx),
            Operation::Chargeback { client, tx } => chargeback(accounts, txs, *client, *tx),
        }
    }
}

/// Append a processed transaction to the store, keyed by its globally unique id.
fn record_tx(txs: &mut HashMap<TxId, StoredTx>, client: ClientId, tx: TxId, amount: Decimal) {
    match txs.entry(tx) {
        Entry::Occupied(_) => log::warn!("duplicate transaction id {tx} for client {client}"),
        Entry::Vacant(entry) => {
            entry.insert(StoredTx {
                client,
                amount,
                disputed: false,
            });
        }
    }
}

/// Look up a stored transaction that `client` owns, logging and returning `None`
/// when it is unknown or belongs to another client (a partner-side error).
fn owned_tx<'a>(
    txs: &'a mut HashMap<TxId, StoredTx>,
    client: ClientId,
    tx: TxId,
    action: &str,
) -> Option<&'a mut StoredTx> {
    match txs.get_mut(&tx) {
        None => {
            log::warn!("{action} references unknown tx {tx} (client {client})");
            None
        }
        Some(stored) if stored.client != client => {
            log::warn!("{action} tx {tx}: not owned by client {client}");
            None
        }
        Some(stored) => Some(stored),
    }
}

/// Hold the disputed funds: available decreases, held increases, total unchanged.
fn dispute(
    accounts: &mut IdOrdMap<Account>,
    txs: &mut HashMap<TxId, StoredTx>,
    client: ClientId,
    tx: TxId,
) {
    let Some(stored) = owned_tx(txs, client, tx, "dispute") else {
        return;
    };
    if stored.disputed {
        log::warn!("dispute tx {tx}: already under dispute");
        return;
    }
    stored.disputed = true;
    let amount = stored.amount;
    account_mut(accounts, client).hold(amount);
    log::debug!("dispute tx {tx}: client {client} holds {amount}");
}

/// Release previously held funds: held decreases, available increases.
fn resolve(
    accounts: &mut IdOrdMap<Account>,
    txs: &mut HashMap<TxId, StoredTx>,
    client: ClientId,
    tx: TxId,
) {
    let Some(stored) = owned_tx(txs, client, tx, "resolve") else {
        return;
    };
    if !stored.disputed {
        log::warn!("resolve tx {tx}: not under dispute");
        return;
    }
    stored.disputed = false;
    let amount = stored.amount;
    account_mut(accounts, client).release(amount);
    log::debug!("resolve tx {tx}: client {client} releases {amount}");
}

/// Reverse the disputed transaction: held and total decrease, account is frozen.
fn chargeback(
    accounts: &mut IdOrdMap<Account>,
    txs: &mut HashMap<TxId, StoredTx>,
    client: ClientId,
    tx: TxId,
) {
    let Some(stored) = owned_tx(txs, client, tx, "chargeback") else {
        return;
    };
    if !stored.disputed {
        log::warn!("chargeback tx {tx}: not under dispute");
        return;
    }
    stored.disputed = false;
    let amount = stored.amount;
    account_mut(accounts, client).chargeback(amount);
    log::debug!("chargeback tx {tx}: client {client} reversed {amount}, account frozen");
}

/// Get the account for `client`, inserting a fresh one if it does not exist.
fn account_mut(accounts: &mut IdOrdMap<Account>, client: ClientId) -> RefMut<'_, Account> {
    if accounts.get(&client).is_none() {
        accounts
            .insert_unique(Account::new(client))
            .expect("account is absent");
    }
    accounts
        .get_mut(&client)
        .expect("account was just inserted")
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

    /// Deserialize one CSV row through serde and convert it, mirroring how the
    /// engine reads records but for a single line in isolation.
    fn deserialize(line: &str) -> Result<Operation> {
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

    #[test]
    fn parses_deposit() {
        assert_eq!(
            deserialize("deposit, 1, 1, 1.0").unwrap(),
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
            deserialize("withdrawal, 2, 5, 3.0").unwrap(),
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
            deserialize("deposit, 1, 6, 5.1234").unwrap(),
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
            deserialize("dispute, 1, 1,").unwrap(),
            Operation::Dispute { client: 1, tx: 1 }
        );
        assert_eq!(
            deserialize("resolve, 1, 1").unwrap(),
            Operation::Resolve { client: 1, tx: 1 }
        );
        assert_eq!(
            deserialize("chargeback, 2, 2,").unwrap(),
            Operation::Chargeback { client: 2, tx: 2 }
        );
    }

    #[test]
    fn errors_on_unknown_type() {
        assert!(deserialize("teleport, 1, 1, 1.0").is_err());
    }

    #[test]
    fn errors_on_missing_amount_for_deposit() {
        assert!(deserialize("deposit, 1, 1,").is_err());
    }

    #[test]
    fn errors_on_non_numeric_amount() {
        assert!(deserialize("deposit, 1, 1, abc").is_err());
    }

    #[test]
    fn errors_on_negative_amount() {
        assert!(deserialize("deposit, 1, 1, -1.0").is_err());
    }

    #[test]
    fn errors_on_invalid_client() {
        assert!(deserialize("deposit, 99999999, 1, 1.0").is_err());
    }

    /// Drive a full dispute lifecycle through `process` and assert both the
    /// account balances and the transaction store after each phase: dispute and
    /// resolve on tx 1, then dispute and chargeback on tx 2.
    #[test]
    fn dispute_resolve_and_chargeback() {
        let mut accounts = IdOrdMap::<Account>::new();
        let mut txs = HashMap::<TxId, StoredTx>::new();

        // Two deposits: available 150, held 0.
        Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        }
        .process(&mut accounts, &mut txs);
        Operation::Deposit {
            client: 1,
            tx: 2,
            amount: dec("50.0"),
        }
        .process(&mut accounts, &mut txs);

        // Dispute tx 1: available 50, held 100, total unchanged at 150.
        Operation::Dispute { client: 1, tx: 1 }.process(&mut accounts, &mut txs);
        let account = accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("50.0"));
        assert_eq!(account.held(), dec("100.0"));
        assert_eq!(account.total(), dec("150.0"));
        assert!(txs[&1].disputed);

        // Resolve tx 1: held funds returned to available.
        Operation::Resolve { client: 1, tx: 1 }.process(&mut accounts, &mut txs);
        let account = accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("150.0"));
        assert_eq!(account.held(), dec("0"));
        assert!(!txs[&1].disputed);

        // Dispute then chargeback tx 2: 50 is reversed and the account freezes.
        Operation::Dispute { client: 1, tx: 2 }.process(&mut accounts, &mut txs);
        Operation::Chargeback { client: 1, tx: 2 }.process(&mut accounts, &mut txs);
        let account = accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("100.0"));
        assert_eq!(account.held(), dec("0"));
        assert_eq!(account.total(), dec("100.0"));
        assert!(account.locked());
        assert!(!txs[&2].disputed);
    }

    /// Disputes referencing an unknown transaction are ignored.
    #[test]
    fn dispute_of_unknown_tx_is_ignored() {
        let mut accounts = IdOrdMap::<Account>::new();
        let mut txs = HashMap::<TxId, StoredTx>::new();

        Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("10.0"),
        }
        .process(&mut accounts, &mut txs);
        Operation::Dispute { client: 1, tx: 99 }.process(&mut accounts, &mut txs);

        let account = accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("10.0"));
        assert_eq!(account.held(), dec("0"));
    }
}
