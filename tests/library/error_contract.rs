use std::error::Error;
use std::io;

use crate::harness::TestContext;

#[test]
fn malformed_configuration_retains_the_toml_source() {
    let ctx = TestContext::new();
    let config = ctx.write_config("version = [");

    let error = grove::validate(Some(config)).expect_err("malformed TOML should fail");

    assert_eq!(error.kind(), grove::AppErrorKind::Configuration);
    let configuration = error.configuration_error().expect("configuration detail should exist");
    assert!(configuration.source().is_some_and(|source| source.is::<toml::de::Error>()));
}

#[test]
fn missing_configuration_retains_the_io_source() {
    let ctx = TestContext::new();
    let missing = ctx.workspace().join("missing.toml");

    let error = grove::validate(Some(missing)).expect_err("a missing config should fail");

    assert_eq!(error.kind(), grove::AppErrorKind::Configuration);
    let configuration = error.configuration_error().expect("configuration detail should exist");
    let source = configuration.source().expect("I/O source should exist");
    assert_eq!(
        source.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::NotFound)
    );
}
