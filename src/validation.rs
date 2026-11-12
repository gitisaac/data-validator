use std::{
    collections::HashSet,
    error::Error,
    fmt,
    io::{self, Read},
    num::ParseIntError,
    path::PathBuf,
};

use csv::StringRecord;

#[derive(Debug)]
pub(crate) enum ConfigurationError {
    InvalidRangeSyntax,
    InvalidMinimum(ParseIntError),
    InvalidMaximum(ParseIntError),
    ReversedBounds,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRangeSyntax => {
                write!(formatter, "expected MIN..=MAX, for example 10..=100")
            }
            Self::InvalidMinimum(source) => {
                write!(formatter, "invalid minimum: {source}")
            }
            Self::InvalidMaximum(source) => {
                write!(formatter, "invalid maximum: {source}")
            }
            Self::ReversedBounds => {
                write!(formatter, "range minimum must not exceed maximum")
            }
        }
    }
}

impl Error for ConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMinimum(source) | Self::InvalidMaximum(source) => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SchemaError {
    MissingColumn(&'static str),
    DuplicateColumn(&'static str),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingColumn(column) => {
                write!(formatter, "missing required column '{column}'")
            }
            Self::DuplicateColumn(column) => {
                write!(formatter, "duplicate column '{column}'")
            }
        }
    }
}

impl Error for SchemaError {}

#[derive(Debug)]
pub(crate) enum ValidationError {
    Schema(SchemaError),
    OpenSource { path: PathBuf, source: io::Error },
    ReadSource(csv::Error),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(source) => write!(formatter, "{source}"),
            Self::OpenSource { path, source } => {
                write!(formatter, "Cannot open '{}': {source}", path.display())
            }
            Self::ReadSource(source) => {
                write!(formatter, "cannot read CSV source: {source}")
            }
        }
    }
}

impl Error for ValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Schema(source) => Some(source),
            Self::OpenSource { source, .. } => Some(source),
            Self::ReadSource(source) => Some(source),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ValidationOptions {
    pub(crate) check_null: bool,
    pub(crate) check_dup_ids: bool,
    pub(crate) range: Option<AmountRange>,
}

#[derive(Debug, Clone)]
pub(crate) struct AmountRange {
    min: u32,
    max: u32,
}

impl AmountRange {
    pub(crate) fn new(min: u32, max: u32) -> Result<Self, ConfigurationError> {
        if min > max {
            return Err(ConfigurationError::ReversedBounds);
        }

        Ok(Self { min, max })
    }

    fn contains(&self, amount: u32) -> bool {
        self.min <= amount && amount <= self.max
    }
}

impl fmt::Display for AmountRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}..={}", self.min, self.max)
    }
}

pub(crate) fn parse_amount_range(value: &str) -> Result<AmountRange, ConfigurationError> {
    let (min, max) = value
        .split_once("..=")
        .ok_or(ConfigurationError::InvalidRangeSyntax)?;

    let min = min
        .parse::<u32>()
        .map_err(ConfigurationError::InvalidMinimum)?;

    let max = max
        .parse::<u32>()
        .map_err(ConfigurationError::InvalidMaximum)?;

    AmountRange::new(min, max)
}

#[derive(Debug, serde::Deserialize)]
struct Transaction {
    id: String,
    amount: String,
}

#[derive(Debug)]
pub(crate) struct Finding {
    pub(crate) line: Option<u64>,
    pub(crate) column: Option<&'static str>,
    pub(crate) message: String,
}

#[derive(Debug)]
pub(crate) struct ValidationReport {
    pub(crate) rows_read: usize,
    pub(crate) invalid_rows: usize,
    pub(crate) findings: Vec<Finding>,
}

fn validate_headers(headers: &StringRecord) -> Result<(), SchemaError> {
    for required in ["id", "amount"] {
        let count = headers.iter().filter(|header| *header == required).count();

        match count {
            0 => {
                return Err(SchemaError::MissingColumn(required));
            }
            1 => {}
            _ => return Err(SchemaError::DuplicateColumn(required)),
        }
    }
    Ok(())
}

fn validate_transaction(
    transaction: &Transaction,
    options: &ValidationOptions,
    seen_ids: &mut HashSet<String>,
    line: Option<u64>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let amount = transaction.amount.trim();
    let transaction_id = transaction.id.trim();

    if options.check_null {
        for (column, value) in [("amount", amount), ("id", transaction_id)] {
            if value.is_empty() {
                findings.push(Finding {
                    message: "value is empty".to_owned(),
                    line,
                    column: Some(column),
                });
            }
        }
    }
    if options.check_dup_ids
        && !transaction_id.is_empty()
        && !seen_ids.insert(transaction_id.to_owned())
    {
        findings.push(Finding {
            message: format!("duplicate ID '{}'", transaction.id),
            line,
            column: Some("id"),
        });
    }
    if !amount.is_empty() {
        match amount.parse::<u32>() {
            Ok(amount) => {
                if let Some(range) = &options.range
                    && !range.contains(amount)
                {
                    findings.push(Finding {
                        message: format!(
                            "amount is out of specified range {0} - {1}",
                            range.min, range.max
                        ),
                        line,
                        column: Some("amount"),
                    });
                }
            }
            Err(error) => findings.push(Finding {
                message: format!("cannot parse amount as an integer: {error}"),
                line,
                column: Some("amount"),
            }),
        }
    }
    findings
}

pub(crate) fn validate_csv(
    reader: impl Read,
    options: &ValidationOptions,
) -> Result<ValidationReport, ValidationError> {
    let mut rdr = csv::Reader::from_reader(reader);
    let headers = rdr.headers().map_err(ValidationError::ReadSource)?.clone();
    validate_headers(&headers).map_err(ValidationError::Schema)?;

    let mut rows_read: usize = 0;
    let mut invalid_rows: usize = 0;
    let mut findings = Vec::<Finding>::new();
    let mut ids = HashSet::<String>::new();
    let rows = rdr.records();
    for row in rows {
        rows_read += 1;

        let row = match row {
            Ok(row) => row,
            Err(error) => match error.kind() {
                csv::ErrorKind::UnequalLengths { .. } | csv::ErrorKind::Utf8 { .. } => {
                    findings.push(Finding {
                        line: error.position().map(|position| position.line()),
                        column: None,
                        message: format!("cannot read record: {error}"),
                    });
                    invalid_rows += 1;
                    continue;
                }
                _ => return Err(ValidationError::ReadSource(error)),
            },
        };
        let line = row.position().map(|position| position.line());
        let transaction = match row.deserialize::<Transaction>(Some(&headers)) {
            Ok(transaction) => transaction,
            Err(err) => {
                let finding = Finding {
                    message: format!("cannot deserialize transaction: {err}"),
                    line,
                    column: None,
                };
                invalid_rows += 1;
                findings.push(finding);
                continue;
            }
        };

        let row_findings = validate_transaction(&transaction, options, &mut ids, line);
        if !row_findings.is_empty() {
            invalid_rows += 1;
        }
        findings.extend(row_findings);
    }
    Ok(ValidationReport {
        rows_read,
        invalid_rows,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation_options() -> ValidationOptions {
        ValidationOptions {
            check_null: true,
            check_dup_ids: true,
            range: None,
        }
    }

    #[test]
    fn numeric_validation_rejects_invalid_unsigned_integers_without_a_range() {
        for amount in ["not-a-number", "-1", "1.5", "4294967296"] {
            let transaction = Transaction {
                id: "unique".to_owned(),
                amount: amount.to_owned(),
            };
            let findings = validate_transaction(
                &transaction,
                &validation_options(),
                &mut HashSet::new(),
                Some(7),
            );

            assert_eq!(findings.len(), 1, "{amount}");
            assert_eq!(findings[0].line, Some(7));
            assert_eq!(findings[0].column, Some("amount"));
            assert!(findings[0].message.contains("cannot parse amount"));
        }
    }

    #[test]
    fn transaction_validation_tracks_trimmed_ids_between_calls() {
        let transaction = Transaction {
            id: " repeat ".to_owned(),
            amount: " 10 ".to_owned(),
        };
        let options = validation_options();
        let mut seen_ids = HashSet::new();

        assert!(
            validate_transaction(&transaction, &options, &mut seen_ids, None).is_empty()
        );
        assert!(seen_ids.contains("repeat"));

        let findings =
            validate_transaction(&transaction, &options, &mut seen_ids, Some(3));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].column, Some("id"));
        assert_eq!(findings[0].line, Some(3));
    }

    #[test]
    fn range_bounds_are_inclusive() {
        let mut options = validation_options();
        options.range = Some(AmountRange::new(10, 100).unwrap());

        for (amount, expected_findings) in [("9", 1), ("10", 0), ("100", 0), ("101", 1)] {
            let transaction = Transaction {
                id: "unique".to_owned(),
                amount: amount.to_owned(),
            };
            let findings =
                validate_transaction(&transaction, &options, &mut HashSet::new(), None);
            assert_eq!(findings.len(), expected_findings, "{amount}");
        }
    }

    #[test]
    fn malformed_records_are_counted_but_do_not_reserve_ids() {
        let input = b"id,amount\nrepeat,10,extra\nrepeat,\xff\nrepeat,20\nrepeat,30\n";
        let report = validate_csv(&input[..], &validation_options()).unwrap();

        assert_eq!(report.rows_read, 4);
        assert_eq!(report.invalid_rows, 3);
        assert_eq!(report.findings.len(), 3);
        assert_eq!(report.findings[0].line, Some(2));
        assert_eq!(report.findings[1].line, Some(3));
        assert!(
            report.findings[..2]
                .iter()
                .all(|finding| finding.column.is_none())
        );
        assert_eq!(report.findings[2].line, Some(5));
        assert_eq!(report.findings[2].column, Some("id"));
    }

    #[test]
    fn duplicate_tracking_is_reset_for_each_csv_source() {
        let input = b"id,amount\nrepeat,10\n";
        for _ in 0..2 {
            let report = validate_csv(&input[..], &validation_options()).unwrap();
            assert_eq!(report.rows_read, 1);
            assert!(report.findings.is_empty());
        }
    }

    #[test]
    fn configuration_errors_identify_invalid_range_components() {
        assert!(matches!(
            parse_amount_range("10..100"),
            Err(ConfigurationError::InvalidRangeSyntax)
        ));
        assert!(matches!(
            parse_amount_range("bad..=100"),
            Err(ConfigurationError::InvalidMinimum(_))
        ));
        assert!(matches!(
            parse_amount_range("10..=bad"),
            Err(ConfigurationError::InvalidMaximum(_))
        ));
        assert!(matches!(
            AmountRange::new(100, 10),
            Err(ConfigurationError::ReversedBounds)
        ));
        let error = parse_amount_range("bad..=100").unwrap_err();
        assert!(error.source().is_some());
    }

    #[test]
    fn schema_errors_identify_missing_and_duplicate_columns() {
        for (input, expected) in [
            ("id\nunique\n", SchemaError::MissingColumn("amount")),
            ("amount\n10\n", SchemaError::MissingColumn("id")),
            ("id,id,amount\n", SchemaError::DuplicateColumn("id")),
            ("id,amount,amount\n", SchemaError::DuplicateColumn("amount")),
        ] {
            let error =
                validate_csv(input.as_bytes(), &validation_options()).unwrap_err();
            assert!(
                matches!(error, ValidationError::Schema(actual) if actual == expected)
            );
        }
    }

    #[test]
    fn source_read_errors_preserve_the_csv_and_io_causes() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("source unavailable"))
            }
        }

        let reader = b"id,amount\nunique,10\n".as_slice().chain(FailingReader);
        let error = validate_csv(reader, &validation_options()).unwrap_err();
        assert!(error.source().is_some());
        match error {
            ValidationError::ReadSource(source) => {
                assert!(matches!(source.kind(), csv::ErrorKind::Io(_)));
                assert!(source.to_string().contains("source unavailable"));
            }
            other => panic!("expected a source-reading error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_reversed_range_bounds() {
        assert!(AmountRange::new(100, 10).is_err());
    }
}
