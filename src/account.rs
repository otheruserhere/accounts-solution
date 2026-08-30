//! A client account and its CSV output row.

use eyre::{Result, bail, eyre};
use iddqd::{IdOrdItem, id_upcast};
use rust_decimal::Decimal;
use serde::Serialize;

use crate::operation::ClientId;

/// A client's account state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    client_id: ClientId,
    available: Decimal,
    held: Decimal,
    locked: bool,
}

impl Account {
    /// A fresh account with zero balances and unlocked.
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            locked: false,
        }
    }

    pub fn available(&self) -> Decimal {
        self.available
    }

    pub fn held(&self) -> Decimal {
        self.held
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    /// Available funds plus funds held for dispute.
    pub fn total(&self) -> Decimal {
        self.available + self.held
    }

    /// Credit the account, erroring if the total balance would overflow.
    ///
    /// `total` is the ceiling to guard, not `available`: held funds can keep
    /// `available` small while `total` sits near the max representable value.
    pub fn deposit(&mut self, amount: Decimal) -> Result<()> {
        if self.total().checked_add(amount).is_none() {
            bail!("deposit would overflow the balance");
        }
        self.available += amount;
        Ok(())
    }

    /// Debit the account, leaving it unchanged and erroring when the available
    /// funds are insufficient.
    pub fn withdraw(&mut self, amount: Decimal) -> Result<()> {
        if self.available < amount {
            bail!(
                "insufficient funds: available {}, requested {amount}",
                self.available
            );
        }
        self.available -= amount;
        Ok(())
    }

    /// Hold funds under dispute: move `amount` from available to held. Errors,
    /// leaving the account unchanged, if either balance would overflow.
    pub fn hold(&mut self, amount: Decimal) -> Result<()> {
        let available = self
            .available
            .checked_sub(amount)
            .ok_or_else(|| eyre!("dispute would overflow the balance"))?;
        let held = self
            .held
            .checked_add(amount)
            .ok_or_else(|| eyre!("dispute would overflow the balance"))?;
        self.available = available;
        self.held = held;
        Ok(())
    }

    /// Release held funds on resolve: move `amount` from held back to available.
    pub fn release(&mut self, amount: Decimal) {
        self.held -= amount;
        self.available += amount;
    }

    /// Reverse a disputed transaction: remove `amount` from held (and total) and
    /// freeze the account.
    pub fn chargeback(&mut self, amount: Decimal) {
        self.held -= amount;
        self.locked = true;
    }
}

impl IdOrdItem for Account {
    type Key<'a> = ClientId;

    fn key(&self) -> Self::Key<'_> {
        self.client_id
    }

    id_upcast!();
}

/// The CSV output row for one account: `client, available, held, total, locked`.
#[derive(Debug, Serialize)]
pub struct Record {
    client: ClientId,
    available: Decimal,
    held: Decimal,
    total: Decimal,
    locked: bool,
}

impl From<&Account> for Record {
    fn from(account: &Account) -> Self {
        Self {
            client: account.client_id,
            available: account.available(),
            held: account.held(),
            total: account.total(),
            locked: account.locked(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// Build an account directly from parts for testing serialization.
    fn account(client_id: ClientId, available: &str, held: &str, locked: bool) -> Account {
        Account {
            client_id,
            available: dec(available),
            held: dec(held),
            locked,
        }
    }

    /// Serialize an account to its (headerless) CSV row.
    fn to_csv(account: &Account) -> String {
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(Vec::new());
        writer.serialize(Record::from(account)).unwrap();
        let bytes = writer.into_inner().unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn serializes_simple_account() {
        let account = account(1, "1.5", "0", false);
        assert_eq!(to_csv(&account), "1,1.5,0,1.5,false\n");
    }

    #[test]
    fn serializes_held_funds_into_total() {
        let account = account(7, "2.0", "3.5", false);
        assert_eq!(to_csv(&account), "7,2.0,3.5,5.5,false\n");
    }

    #[test]
    fn serializes_locked_account() {
        let account = account(2, "0", "0", true);
        assert_eq!(to_csv(&account), "2,0,0,0,true\n");
    }

    #[test]
    fn serializes_four_decimal_precision() {
        let account = account(3, "5.1234", "0", false);
        assert_eq!(to_csv(&account), "3,5.1234,0,5.1234,false\n");
    }

    #[test]
    fn deposit_then_withdraw() {
        let mut account = Account::new(1);
        account.deposit(dec("3.0")).unwrap();
        assert!(account.withdraw(dec("1.5")).is_ok());
        assert_eq!(account.available, dec("1.5"));
        assert_eq!(account.total(), dec("1.5"));
    }

    #[test]
    fn withdraw_with_insufficient_funds_is_ignored() {
        let mut account = Account::new(1);
        account.deposit(dec("2.0")).unwrap();
        assert!(account.withdraw(dec("3.0")).is_err());
        assert_eq!(account.available, dec("2.0"));
    }
}
