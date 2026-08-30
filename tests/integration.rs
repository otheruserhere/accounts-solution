use std::process::Command;

/// End-to-end check: the engine processes the sample transactions and writes the
/// resulting account balances to stdout, ordered by client id.
///
/// Only deposits and withdrawals are applied so far; the dispute, resolve, and
/// chargeback rows in the fixture are no-ops. Client 2's `3.0` withdrawal fails
/// for insufficient funds, leaving its balance at the `2.0` deposited.
#[test]
fn outputs_account_balances() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.csv");

    let output = Command::new(env!("CARGO_BIN_EXE_accounts-solution"))
        .arg(fixture)
        .output()
        .expect("run binary");

    assert!(output.status.success(), "binary exited with failure");

    let expected = "client,available,held,total,locked\n\
                    1,6.6234,0,6.6234,false\n\
                    2,2,0,2,false\n";
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

/// Malformed rows must be skipped without aborting the run: an unknown
/// operation type and a row with an empty column are both dropped, while the
/// two valid deposits (clients 1 and 3) are processed.
#[test]
fn malformed_row_does_not_abort_processing() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/malformed.csv");

    let output = Command::new(env!("CARGO_BIN_EXE_accounts-solution"))
        .arg(fixture)
        .output()
        .expect("run binary");

    assert!(output.status.success(), "binary exited with failure");

    let expected = "client,available,held,total,locked\n\
                    1,1,0,1,false\n\
                    3,3,0,3,false\n";
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
