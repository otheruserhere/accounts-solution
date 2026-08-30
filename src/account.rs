//! A client account and its CSV output row.

use rust_decimal::Decimal;
use serde::Serialize;

use crate::operation::ClientId;

/// A client's account state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    client: ClientId,
    available: Decimal,
    held: Decimal,
    locked: bool,
}

impl Account {
    /// A fresh account with zero balances and unlocked.
    pub fn new(client: ClientId) -> Self {
        Self {
            client,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            locked: false,
        }
    }

    /// Available funds plus funds held for dispute.
    pub fn total(&self) -> Decimal {
        self.available + self.held
    }
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
            client: account.client,
            available: account.available,
            held: account.held,
            total: account.total(),
            locked: account.locked,
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
    fn account(client: ClientId, available: &str, held: &str, locked: bool) -> Account {
        Account {
            client,
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
}
