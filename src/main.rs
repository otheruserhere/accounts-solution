mod account;
mod operation;

use std::process::ExitCode;

use eyre::{Result, WrapErr, eyre};

use operation::{Operation, Record};

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

    // `from_path` reads through a buffered file handle and `deserialize` yields
    // one record at a time, so the input is streamed rather than held in memory.
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_path(&path)
        .wrap_err_with(|| format!("failed to open {}", path.to_string_lossy()))?;

    for result in reader.deserialize() {
        let record: Record = result.wrap_err("failed to parse row")?;
        let operation = Operation::try_from(record)?;
        dbg!(&operation);
    }

    Ok(())
}
