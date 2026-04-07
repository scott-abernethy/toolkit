use serde::Serialize;
use std::process;

/// Standard JSON error output for agent consumption.
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Print a JSON error to stdout and exit with code 1.
pub fn exit_with_error(msg: impl Into<String>) -> ! {
    let resp = ErrorResponse {
        error: msg.into(),
    };
    println!("{}", serde_json::to_string(&resp).unwrap());
    process::exit(1)
}
