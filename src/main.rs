mod operation;

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let path = match std::env::args_os().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: accounts-solution <transactions.csv>");
            return ExitCode::FAILURE;
        }
    };

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("error: failed to read {}: {err}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = io::stdout().write_all(&bytes) {
        eprintln!("error: failed to write output: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
