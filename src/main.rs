mod account;
mod ledger;
mod operation;
mod transaction;

use std::io;
use std::process::ExitCode;

use eyre::{Result, WrapErr, eyre};

use account::Record as AccountRecord;
use ledger::Ledger;
use operation::{Operation, Record as InputRecord};

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

    // Stream one record at a time rather than loading the whole file.
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_path(&path)
        .wrap_err_with(|| format!("failed to open {}", path.to_string_lossy()))?;

    // A malformed row is logged and skipped so the rest of the file still runs.
    let mut ledger = Ledger::new();
    for result in reader.deserialize() {
        let operation = result
            .map_err(eyre::Report::from)
            .and_then(|record: InputRecord| Operation::try_from(record));
        match operation {
            Ok(operation) => ledger.apply(operation),
            Err(err) => log::error!("failed to parse row: {err}"),
        }
    }

    let mut writer = csv::Writer::from_writer(io::stdout().lock());
    let mut count = 0;
    for account in ledger.accounts() {
        writer
            .serialize(AccountRecord::from(account))
            .wrap_err("failed to write account")?;
        count += 1;
    }
    writer.flush().wrap_err("failed to flush output")?;

    log::info!("wrote {count} accounts");
    Ok(())
}
