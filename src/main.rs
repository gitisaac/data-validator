mod validation;

use std::{fs::File, path::PathBuf, process::ExitCode};

use validation::{
    AmountRange, ValidationError, ValidationOptions, ValidationReport,
    parse_amount_range, validate_csv,
};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)] // Read from `Cargo.toml``
struct Cli {
    /// The path of the file to validate
    #[arg(short, long)]
    file: PathBuf,

    // Options
    /// Disable null checking: enabled by default
    #[arg(short = 'n', long = "no-null-check", action = clap::ArgAction::SetFalse)]
    check_null: bool,

    /// Disable checking for duplicate IDs, enabled by default
    #[arg(short = 'd', long = "no-dup-ids-check", action = clap::ArgAction::SetFalse)]
    check_dup_ids: bool,

    /// Optional inclusive range; nonempty amounts are always checked as u32 integers
    #[arg(
        short,
        long,
        action = clap::ArgAction::Set,
        value_name = "MIN..=MAX",
        value_parser = parse_amount_range
    )]
    range: Option<AmountRange>,
}

fn print_report(cli: &Cli, report: &ValidationReport) {
    let range = match &cli.range {
        Some(range) => range.to_string(),
        None => "disabled".to_owned(),
    };

    println!("File: {}", cli.file.display());
    println!(
        concat!(
            "Checks: empty fields: {}; duplicate IDs: {}; ",
            "numeric amounts: enabled; amount range: {}"
        ),
        if cli.check_null {
            "enabled"
        } else {
            "disabled"
        },
        if cli.check_dup_ids {
            "enabled"
        } else {
            "disabled"
        },
        range,
    );
    println!();

    if report.findings.is_empty() {
        println!("No issues found with the enabled checks.");
    } else {
        for finding in &report.findings {
            match finding.line {
                Some(line) => print!("Line {line}"),
                None => print!("Unknown line"),
            }
            if let Some(column) = finding.column {
                print!(", {column}");
            }
            println!(": {}", finding.message);
        }
    }

    println!(
        "\n{} records read; {} invalid records; {} findings.",
        report.rows_read,
        report.invalid_rows,
        report.findings.len(),
    );
}

fn run(cli: &Cli) -> Result<ValidationReport, ValidationError> {
    let file = File::open(&cli.file).map_err(|source| ValidationError::OpenSource {
        path: cli.file.clone(),
        source,
    })?;
    let options = ValidationOptions {
        check_null: cli.check_null,
        check_dup_ids: cli.check_dup_ids,
        range: cli.range.clone(),
    };
    validate_csv(file, &options)
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(report) => {
            print_report(&cli, &report);
            if report.findings.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn source_open_errors_preserve_the_path_and_io_cause() {
        let mut cli = cli_for_empty_fields();
        cli.file.set_file_name("not-present.csv");
        let error = run(&cli).unwrap_err();

        assert!(error.source().is_some());
        match error {
            ValidationError::OpenSource { path, source } => {
                assert_eq!(path, cli.file);
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected a source-opening error, got {other:?}"),
        }
    }

    fn cli_for_empty_fields() -> Cli {
        Cli {
            file: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/empty-fields.csv"),
            check_null: true,
            check_dup_ids: true,
            range: Some(AmountRange::new(0, 100).unwrap()),
        }
    }

    #[test]
    fn empty_fields_do_not_produce_duplicate_or_numeric_findings() {
        let report = run(&cli_for_empty_fields()).unwrap();

        assert_eq!(report.rows_read, 5);
        assert_eq!(report.findings.len(), 7);
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|finding| finding.column == Some("id"))
                .count(),
            4
        );
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|finding| finding.column == Some("amount"))
                .count(),
            3
        );
        let duplicate = report.findings.last().unwrap();
        assert_eq!(duplicate.line, Some(6));
        assert!(duplicate.message.contains("repeat"));
    }

    #[test]
    fn disabling_null_checks_does_not_turn_empty_values_into_other_findings() {
        let mut cli = cli_for_empty_fields();
        cli.check_null = false;

        let report = run(&cli).unwrap();

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].column, Some("id"));
        assert!(report.findings[0].message.contains("repeat"));
    }
}
