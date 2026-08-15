// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `volumetrace` feature.

use std::path::PathBuf;

use super as volumetrace;
use crate::shared::cli::{first_positional, has_flag};
use crate::shared::util::{Error, Result};

pub const HELP: &str = r#"
    volume trace  trace poop [PATH] [--dry-run]
                  trace clean [PATH] [--dry-run]"#;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new(
            "usage: mosbsfol trace <poop|clean> [PATH] [--dry-run]",
        ));
    }
    let path = PathBuf::from(first_positional(&args[1..], true).unwrap_or_else(|| ".".into()));
    let dry_run = has_flag(&args[1..], &["--dry-run"]);
    match args[0].as_str() {
        "poop" | "make" => {
            for p in volumetrace::poop_volume(&path, dry_run)? {
                println!("💩 created {}", p.display());
            }
            Ok(())
        }
        "clean" => {
            for p in volumetrace::clean_volume(&path, dry_run)? {
                println!("removed {}", p.display());
            }
            Ok(())
        }
        other => Err(Error::new(format!(
            "unknown trace subcommand {other:?} (poop|clean)"
        ))),
    }
}
