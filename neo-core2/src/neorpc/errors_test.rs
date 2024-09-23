use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::io;
use std::result::Result;

use anyhow::Result as AnyResult;
use thiserror::Error;
use assert_matches::assert_matches;

#[derive(Debug, Error)]
#[error("Internal error (-32603) - {0}")]
struct InternalServerError(String);

#[derive(Debug, Error)]
#[error("Invalid params error - {0}")]
struct InvalidParamsError(String);

fn new_internal_server_error(msg: &str) -> InternalServerError {
    InternalServerError(msg.to_string())
}

fn new_invalid_params_error(msg: &str) -> InvalidParamsError {
    InvalidParamsError(msg.to_string())
}

#[test]
fn test_error_errors_as() -> AnyResult<()> {
    let err = new_internal_server_error("some error");
    let wrapped = format!("some meaningful error: {}", err);

    // Check that Error can be used as a target for errors.As:
    let actual: Option<&InternalServerError> = wrapped.as_str().downcast_ref();
    assert!(actual.is_some());
    assert_eq!(actual.unwrap().to_string(), "Internal error (-32603) - some error");

    let bad: Option<&io::Error> = wrapped.as_str().downcast_ref();
    assert!(bad.is_none());

    Ok(())
}

#[test]
fn test_error_errors_is() -> AnyResult<()> {
    let err = new_internal_server_error("some error");
    let wrapped = format!("some meaningful error: {}", err);

    // Check that Error can be recognized via errors.Is:
    let ref_err = new_internal_server_error("another server error");
    assert!(wrapped.contains(&ref_err.to_string()));

    // Invalid target type:
    let invalid_err = new_invalid_params_error("invalid params");
    assert!(!wrapped.contains(&invalid_err.to_string()));

    Ok(())
}
