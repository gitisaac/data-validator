use std::{path::PathBuf, process::Command};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn findings_include_locations_and_count_each_invalid_record_once() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-validator"))
        .arg("--file")
        .arg(fixture("empty-fields.csv"))
        .args(["--range", "0..=100"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("empty-fields.csv"));
    assert!(report.contains("Line 2, amount: value is empty"));
    assert!(report.contains("Line 2, id: value is empty"));
    assert!(report.contains("Line 6, id: duplicate ID 'repeat'"));
    assert!(report.contains("5 records read; 4 invalid records; 7 findings"));
}

#[test]
fn clean_input_prints_success_and_enabled_checks() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-validator"))
        .arg("--file")
        .arg(fixture("valid.csv"))
        .args(["--range", "10..=100"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("empty fields: enabled"));
    assert!(report.contains("duplicate IDs: enabled"));
    assert!(report.contains("amount range: 10..=100"));
    assert!(report.contains("No issues found with the enabled checks."));
    assert!(report.contains("2 records read; 0 invalid records; 0 findings"));
}

#[test]
fn fatal_and_argument_errors_go_to_stderr_without_a_success_report() {
    for (file, args, expected, exit_code) in [
        ("not-present.csv", Vec::new(), "Cannot open", 1),
        (
            "missing-headers.csv",
            Vec::new(),
            "missing required column 'amount'",
            1,
        ),
        ("valid.csv", vec!["--range", "100..=10"], "range minimum", 2),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_data-validator"))
            .arg("--file")
            .arg(fixture(file))
            .args(args)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(exit_code), "{file}");
        assert!(output.stdout.is_empty(), "{file}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.starts_with("error: "), "{error}");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn numeric_errors_and_out_of_range_amounts_are_included_in_the_summary() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-validator"))
        .arg("--file")
        .arg(fixture("invalid-amounts.csv"))
        .args(["--range", "10..=100"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("Line 2, amount: cannot parse amount"));
    assert!(report.contains("Line 3, id: duplicate ID 'repeat'"));
    assert!(report.contains("Line 3, amount: amount is out of specified range"));
    assert!(report.contains("2 records read; 2 invalid records; 3 findings"));
}

#[test]
fn disabled_checks_are_identified_in_a_clean_report() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-validator"))
        .arg("--file")
        .arg(fixture("empty-fields.csv"))
        .args(["--no-null-check", "--no-dup-ids-check"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("empty fields: disabled"));
    assert!(report.contains("duplicate IDs: disabled"));
    assert!(report.contains("amount range: disabled"));
    assert!(report.contains("No issues found with the enabled checks."));
    assert!(report.contains("5 records read; 0 invalid records; 0 findings"));
}

#[test]
fn numeric_errors_are_reported_without_a_range_or_optional_checks() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-validator"))
        .arg("--file")
        .arg(fixture("invalid-amounts.csv"))
        .args(["--no-null-check", "--no-dup-ids-check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("Line 2, amount: cannot parse amount"));
    assert!(report.contains("amount range: disabled"));
    assert!(report.contains("numeric amounts: enabled"));
    assert!(!report.contains("out of specified range"));
    assert!(report.contains("2 records read; 1 invalid records; 1 findings"));
}
