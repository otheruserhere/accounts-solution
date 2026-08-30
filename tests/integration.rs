use std::process::Command;

/// The binary currently echoes its input file to stdout unchanged.
/// This guards that pass-through contract byte-for-byte against the fixture.
#[test]
fn outputs_input_unchanged() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.csv");
    let expected = std::fs::read(fixture).expect("read fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_accounts-solution"))
        .arg(fixture)
        .output()
        .expect("run binary");

    assert!(output.status.success(), "binary exited with failure");
    assert_eq!(output.stdout, expected, "stdout differs from fixture body");
}
