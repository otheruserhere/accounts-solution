//! The engine state: client accounts and the deposit store, mutated by applying
//! [`Operation`]s.

use std::collections::HashMap;

use iddqd::IdOrdMap;
use iddqd::id_ord_map::RefMut;
use rust_decimal::Decimal;

use crate::account::Account;
use crate::operation::{ClientId, Operation, TxId};
use crate::transaction::{DisputeState, StoredTx};

/// Accumulates account balances as operations are applied.
#[derive(Debug)]
pub struct Ledger {
    accounts: IdOrdMap<Account>,
    deposits: HashMap<TxId, StoredTx>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            accounts: IdOrdMap::new(),
            deposits: HashMap::new(),
        }
    }

    /// Apply one operation, mutating account balances and the deposit store.
    ///
    /// A frozen account rejects every operation, so a locked account's state is
    /// final.
    pub fn apply(&mut self, op: Operation) {
        if self.is_locked(op.client()) {
            log::warn!("account {} is locked; ignoring {op:?}", op.client());
            return;
        }
        match op {
            Operation::Deposit { client, tx, amount } => {
                if self.deposits.contains_key(&tx) {
                    log::warn!("duplicate transaction id {tx} for client {client}");
                    return;
                }
                let result = self.account_mut(client).deposit(amount);
                match result {
                    Ok(()) => {
                        self.record_deposit(client, tx, amount);
                        log::debug!("deposit tx {tx}: client {client} credited {amount}");
                    }
                    Err(err) => {
                        log::warn!("deposit tx {tx} for client {client} failed: {err}");
                    }
                }
            }
            Operation::Withdrawal { client, tx, amount } => {
                let result = self.account_mut(client).withdraw(amount);
                match result {
                    Ok(()) => {
                        log::debug!("withdrawal tx {tx}: client {client} debited {amount}");
                    }
                    Err(err) => {
                        log::warn!("withdrawal tx {tx} for client {client} failed: {err}");
                    }
                }
            }
            Operation::Dispute { client, tx } => self.dispute(client, tx),
            Operation::Resolve { client, tx } => self.resolve(client, tx),
            Operation::Chargeback { client, tx } => self.chargeback(client, tx),
        }
    }

    /// Accounts in client-id order, for producing the resulting CSV.
    pub fn accounts(&self) -> impl Iterator<Item = &Account> {
        self.accounts.iter()
    }

    /// Whether the client's account exists and is frozen.
    fn is_locked(&self, client: ClientId) -> bool {
        self.accounts.get(&client).is_some_and(Account::locked)
    }

    /// Get the account for `client`, inserting a fresh one if it does not exist.
    fn account_mut(&mut self, client: ClientId) -> RefMut<'_, Account> {
        if self.accounts.get(&client).is_none() {
            self.accounts
                .insert_unique(Account::new(client))
                .expect("account is absent");
        }
        self.accounts
            .get_mut(&client)
            .expect("account was just inserted")
    }

    /// Record a credited deposit, keyed by its unique tx id, for dispute handling.
    fn record_deposit(&mut self, client: ClientId, tx: TxId, amount: Decimal) {
        self.deposits.insert(
            tx,
            StoredTx {
                client,
                amount,
                state: DisputeState::Undisputed,
            },
        );
    }

    /// Hold the disputed funds: available decreases, held increases, total same.
    fn dispute(&mut self, client: ClientId, tx: TxId) {
        let Some(stored) = owned_tx(&mut self.deposits, client, tx, "dispute") else {
            return;
        };
        if stored.state != DisputeState::Undisputed {
            log::warn!("dispute tx {tx}: not open for dispute ({:?})", stored.state);
            return;
        }
        stored.state = DisputeState::Disputed;
        let amount = stored.amount;
        self.account_mut(client).hold(amount);
        log::debug!("dispute tx {tx}: client {client} holds {amount}");
    }

    /// Release previously held funds: held decreases, available increases.
    fn resolve(&mut self, client: ClientId, tx: TxId) {
        let Some(stored) = owned_tx(&mut self.deposits, client, tx, "resolve") else {
            return;
        };
        if stored.state != DisputeState::Disputed {
            log::warn!("resolve tx {tx}: not under dispute");
            return;
        }
        stored.state = DisputeState::Undisputed;
        let amount = stored.amount;
        self.account_mut(client).release(amount);
        log::debug!("resolve tx {tx}: client {client} releases {amount}");
    }

    /// Reverse the disputed transaction: held and total decrease, account frozen.
    fn chargeback(&mut self, client: ClientId, tx: TxId) {
        let Some(stored) = owned_tx(&mut self.deposits, client, tx, "chargeback") else {
            return;
        };
        if stored.state != DisputeState::Disputed {
            log::warn!("chargeback tx {tx}: not under dispute");
            return;
        }
        stored.state = DisputeState::ChargedBack;
        let amount = stored.amount;
        self.account_mut(client).chargeback(amount);
        log::debug!("chargeback tx {tx}: client {client} reversed {amount}, account frozen");
    }
}

/// Look up a stored transaction that `client` owns, logging and returning `None`
/// when it is unknown or belongs to another client (a partner-side error).
fn owned_tx<'a>(
    deposits: &'a mut HashMap<TxId, StoredTx>,
    client: ClientId,
    tx: TxId,
    action: &str,
) -> Option<&'a mut StoredTx> {
    match deposits.get_mut(&tx) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// Drive a full dispute lifecycle and assert both the account balances and
    /// the transaction store after each phase: dispute and resolve on tx 1, then
    /// dispute and chargeback on tx 2.
    #[test]
    fn dispute_resolve_and_chargeback() {
        let mut ledger = Ledger::new();

        // Two deposits: available 150, held 0.
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        });
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 2,
            amount: dec("50.0"),
        });

        // Dispute tx 1: available 50, held 100, total unchanged at 150.
        ledger.apply(Operation::Dispute { client: 1, tx: 1 });
        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("50.0"));
        assert_eq!(account.held(), dec("100.0"));
        assert_eq!(account.total(), dec("150.0"));
        assert_eq!(ledger.deposits[&1].state, DisputeState::Disputed);

        // Resolve tx 1: held funds returned to available.
        ledger.apply(Operation::Resolve { client: 1, tx: 1 });
        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("150.0"));
        assert_eq!(account.held(), dec("0"));
        assert_eq!(ledger.deposits[&1].state, DisputeState::Undisputed);

        // Dispute then chargeback tx 2: 50 is reversed and the account freezes.
        ledger.apply(Operation::Dispute { client: 1, tx: 2 });
        ledger.apply(Operation::Chargeback { client: 1, tx: 2 });
        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("100.0"));
        assert_eq!(account.held(), dec("0"));
        assert_eq!(account.total(), dec("100.0"));
        assert!(account.locked());
        assert_eq!(ledger.deposits[&2].state, DisputeState::ChargedBack);
    }

    /// A charged-back transaction is terminal: re-disputing and charging it back
    /// again must be ignored, not double-reverse the funds.
    #[test]
    fn cannot_chargeback_twice() {
        let mut ledger = Ledger::new();

        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        });
        ledger.apply(Operation::Dispute { client: 1, tx: 1 });
        ledger.apply(Operation::Chargeback { client: 1, tx: 1 });

        // Second dispute + chargeback on the same tx should have no effect.
        ledger.apply(Operation::Dispute { client: 1, tx: 1 });
        ledger.apply(Operation::Chargeback { client: 1, tx: 1 });

        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("0"));
        assert_eq!(account.held(), dec("0"));
        assert_eq!(account.total(), dec("0"));
        assert!(account.locked());
    }

    /// A locked account rejects further withdrawals, leaving its balance intact.
    #[test]
    fn locked_account_cannot_withdraw() {
        let mut ledger = Ledger::new();

        // Deposit 150, then chargeback tx 1 (100) to lock the account with 50 left.
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        });
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 2,
            amount: dec("50.0"),
        });
        ledger.apply(Operation::Dispute { client: 1, tx: 1 });
        ledger.apply(Operation::Chargeback { client: 1, tx: 1 });

        ledger.apply(Operation::Withdrawal {
            client: 1,
            tx: 3,
            amount: dec("20.0"),
        });

        let account = ledger.accounts.get(&1).unwrap();
        assert!(account.locked());
        assert_eq!(account.available(), dec("50.0"));
    }

    /// Disputes referencing an unknown transaction are ignored.
    #[test]
    fn dispute_of_unknown_tx_is_ignored() {
        let mut ledger = Ledger::new();

        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("10.0"),
        });
        ledger.apply(Operation::Dispute { client: 1, tx: 99 });

        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("10.0"));
        assert_eq!(account.held(), dec("0"));
    }

    /// A frozen account rejects every operation, including deposits.
    #[test]
    fn locked_account_ignores_all_operations() {
        let mut ledger = Ledger::new();

        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        });
        ledger.apply(Operation::Dispute { client: 1, tx: 1 });
        ledger.apply(Operation::Chargeback { client: 1, tx: 1 });

        // The account is now frozen; a further deposit must be ignored.
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 2,
            amount: dec("50.0"),
        });

        let account = ledger.accounts.get(&1).unwrap();
        assert!(account.locked());
        assert_eq!(account.available(), dec("0"));
        assert_eq!(account.total(), dec("0"));
    }

    /// A second deposit reusing an existing tx id is ignored, not applied twice.
    #[test]
    fn duplicate_deposit_is_ignored() {
        let mut ledger = Ledger::new();

        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        });
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: dec("100.0"),
        });

        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), dec("100.0"));
        assert_eq!(account.total(), dec("100.0"));
    }

    /// A deposit that would overflow the balance is ignored, not panicking.
    #[test]
    fn deposit_overflow_is_ignored() {
        let mut ledger = Ledger::new();

        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 1,
            amount: Decimal::MAX,
        });
        ledger.apply(Operation::Deposit {
            client: 1,
            tx: 2,
            amount: Decimal::MAX,
        });

        let account = ledger.accounts.get(&1).unwrap();
        assert_eq!(account.available(), Decimal::MAX);
        assert_eq!(account.total(), Decimal::MAX);
    }
}
