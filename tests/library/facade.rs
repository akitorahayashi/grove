use std::path::PathBuf;

#[test]
fn root_facade_exposes_supported_use_cases_and_reports() {
    let _: fn(Option<PathBuf>, Vec<String>, bool) -> Result<grove::RefreshReport, grove::AppError> =
        grove::refresh;
    let _: fn(Option<PathBuf>, Vec<String>, bool) -> Result<grove::StatusReport, grove::AppError> =
        grove::status;
    let _: fn(Option<PathBuf>, Vec<String>, bool) -> Result<grove::SyncReport, grove::AppError> =
        grove::sync;
    let _: fn(Option<PathBuf>) -> Result<grove::ValidationReport, grove::AppError> =
        grove::validate;
    let _: fn() -> std::process::ExitCode = grove::cli;
    let options = grove::RefreshOptions::new(true);
    assert!(options.dry_run());
}
