mod account;
mod operation;
mod transaction;

use std::collections::HashMap;
use std::io;
use std::process::ExitCode;

use eyre::{Result, WrapErr, eyre};
use iddqd::IdOrdMap;

use account::{Account, Record as AccountRecord};
use operation::{Operation, Record as InputRecord, TxId};
use transaction::StoredTx;

fn main() -> ExitCode {
    env_logger::init();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:?}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| eyre!("usage: accounts-solution <transactions.csv>"))?;

    log::info!("processing transactions from {}", path.to_string_lossy());

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_path(&path)
        .wrap_err_with(|| format!("failed to open {}", path.to_string_lossy()))?;

    let mut accounts = IdOrdMap::<Account>::new();
    let mut txs = HashMap::<TxId, StoredTx>::new();
    for result in reader.deserialize() {
        let operation = result
            .map_err(eyre::Report::from)
            .and_then(|record: InputRecord| Operation::try_from(record));
        match operation {
            Ok(operation) => operation.process(&mut accounts, &mut txs),
            Err(err) => log::error!("failed to parse row: {err}"),
        }
    }

    dbg!(&txs);

    let mut writer = csv::Writer::from_writer(io::stdout().lock());
    for account in &accounts {
        writer
            .serialize(AccountRecord::from(account))
            .wrap_err("failed to write account")?;
    }
    writer.flush().wrap_err("failed to flush output")?;

    log::info!("wrote {} accounts", accounts.len());
    Ok(())
}
