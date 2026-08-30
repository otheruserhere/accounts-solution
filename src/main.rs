mod account;
mod operation;

use std::io;
use std::process::ExitCode;

use eyre::{Result, WrapErr, eyre};
use iddqd::IdOrdMap;

use account::{Account, Record as AccountRecord};
use operation::{Operation, Record as InputRecord};

fn main() -> ExitCode {
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

    // Stream one record at a time rather than loading the whole file.
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_path(&path)
        .wrap_err_with(|| format!("failed to open {}", path.to_string_lossy()))?;

    let mut accounts = IdOrdMap::<Account>::new();
    for result in reader.deserialize() {
        let record: InputRecord = result.wrap_err("failed to parse row")?;
        Operation::try_from(record)?.process(&mut accounts);
    }

    // IdOrdMap iterates in client id order, so output is deterministic.
    let mut writer = csv::Writer::from_writer(io::stdout().lock());
    for account in &accounts {
        writer
            .serialize(AccountRecord::from(account))
            .wrap_err("failed to write account")?;
    }
    writer.flush().wrap_err("failed to flush output")?;

    Ok(())
}
