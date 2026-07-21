use std::path::PathBuf;

#[test]
fn root_facade_exposes_supported_use_cases_and_reports() {
    let _: fn(
        Option<PathBuf>,
        Vec<String>,
        grove::RefreshOptions,
    ) -> Result<grove::RefreshReport, grove::AppError> = grove::refresh;
    let _: fn(Option<PathBuf>, Vec<String>, bool) -> Result<grove::StatusReport, grove::AppError> =
        grove::status;
    let _: fn(
        Option<PathBuf>,
        Vec<String>,
        grove::SyncOptions,
    ) -> Result<grove::SyncReport, grove::AppError> = grove::sync;
    let _: fn(Option<PathBuf>) -> Result<grove::ValidationReport, grove::AppError> =
        grove::validate;
    let _: fn(String, Option<PathBuf>) -> Result<grove::CloneReport, grove::AppError> =
        grove::clone;
    let _: fn() -> std::process::ExitCode = grove::cli;

    let refresh_options = grove::RefreshOptions::new(true);
    assert!(refresh_options.dry_run());

    let sync_options = grove::SyncOptions::new(true, true);
    assert!(sync_options.dry_run());
    assert!(sync_options.register_zoxide());
}
