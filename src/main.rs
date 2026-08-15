// SPDX-License-Identifier: Apache-2.0

//! MOSBSFOL bootstrap: collect args, delegate to the application core.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match mosbsfol::core::cli::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mosbsfol: error: {e}");
            ExitCode::FAILURE
        }
    }
}
