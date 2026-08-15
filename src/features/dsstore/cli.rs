// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `dsstore` feature.

use std::path::PathBuf;

use super::finder::{self, FinderOptions};
use super::format::{display_record, records_sorted_cmp, DsStore};
use crate::shared::cli::{first_positional, has_flag, need};
use crate::shared::util::{Error, Result};

pub const HELP: &str = r#"
    .DS_Store     dsstore poop [PATH] [-r] [--skip-hidden] [--dry-run]
                  dsstore inspect FILE
                  dsstore clean [PATH] [-r] [--dry-run]"#;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new(
            "usage: mosbsfol dsstore <poop|inspect|clean> ...",
        ));
    }
    match args[0].as_str() {
        "poop" | "make" => {
            let path =
                PathBuf::from(first_positional(&args[1..], true).unwrap_or_else(|| ".".into()));
            let recursive = has_flag(&args[1..], &["-r", "--recursive"]);
            let skip_hidden = has_flag(&args[1..], &["--skip-hidden"]);
            let dry_run = has_flag(&args[1..], &["--dry-run"]);
            let opts = FinderOptions::default();
            let files = finder::poop_tree(&path, &opts, recursive, skip_hidden, dry_run)?;
            for f in &files {
                if dry_run {
                    println!("would create {}", f.display());
                } else {
                    println!("💩 created {}", f.display());
                }
            }
            if files.is_empty() {
                println!("nothing to do (is this a directory?)");
            }
            Ok(())
        }
        "inspect" | "read" => {
            let file = need(&args[1..], 0, "path to .DS_Store")?;
            let bytes = std::fs::read(&file)?;
            let parsed = DsStore::parse(&bytes)?;
            println!(
                "{}: .DS_Store, {} records, allocator @0x{:x} size 0x{:x}, {} blocks",
                file,
                parsed.record_count(),
                parsed.allocator_offset,
                parsed.allocator_size,
                parsed.block_count
            );
            let mut records = parsed.records.clone();
            records.sort_by(records_sorted_cmp);
            for r in records {
                println!("  {}", display_record(&r));
            }
            Ok(())
        }
        "clean" => {
            let path =
                PathBuf::from(first_positional(&args[1..], true).unwrap_or_else(|| ".".into()));
            let recursive = has_flag(&args[1..], &["-r", "--recursive"]);
            let dry_run = has_flag(&args[1..], &["--dry-run"]);
            let files = finder::clean_tree(&path, recursive, dry_run)?;
            for f in &files {
                println!("removed {}", f.display());
            }
            if files.is_empty() {
                println!("no .DS_Store files found");
            }
            Ok(())
        }
        other => Err(Error::new(format!(
            "unknown dsstore subcommand {other:?} (poop|inspect|clean)"
        ))),
    }
}
