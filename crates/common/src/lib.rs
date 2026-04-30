pub mod client;
pub mod config;
pub mod error;
pub mod key;
pub mod protocol;
pub mod sql;

use serde::Serialize;
use std::process;

pub use config::{load_named_section, load_named_section_with_name, load_section};
pub use error::{Result, ToolkitError};

/// Standard JSON error output for agent consumption.
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Print a `ToolkitError` as JSON to stdout and exit with code 1.
///
/// This is the binary-entrypoint helper. Library code should propagate
/// `Result<T, ToolkitError>` and let `main` decide when to exit.
pub fn exit_with_error(err: ToolkitError) -> ! {
    let resp = ErrorResponse {
        error: err.message().to_string(),
    };
    println!("{}", serde_json::to_string(&resp).unwrap());
    process::exit(1)
}
