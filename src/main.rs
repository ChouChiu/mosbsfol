// SPDX-License-Identifier: Apache-2.0

//! MOSBSFOL bootstrap: collect args without UTF-8 loss, delegate to the
//! application core.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match mosbsfol::core::cli::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mosbsfol: error: {e}");
            ExitCode::FAILURE
        }
    }
}
