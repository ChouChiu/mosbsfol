// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `maczip` feature.

use std::path::{Path, PathBuf};

use clap::{value_parser, Arg, ArgMatches, Command};

use super as maczip;
use crate::shared::cli::dry_run_flag;
use crate::shared::util::Result;

pub fn command() -> Command {
    Command::new("maczip")
        .about("Create a Finder-style ZIP with __MACOSX AppleDouble entries")
        .alias("zip")
        .arg(
            Arg::new("dir")
                .value_parser(value_parser!(PathBuf))
                .required(true)
                .value_name("DIR")
                .help("Directory to archive"),
        )
        .arg(
            Arg::new("output")
                .value_parser(value_parser!(PathBuf))
                .required(false)
                .value_name("OUT.zip")
                .help("Output ZIP path (defaults to DIR.zip next to DIR)"),
        )
        .arg(dry_run_flag())
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    let dir = matches
        .get_one::<PathBuf>("dir")
        .expect("clap requires DIR")
        .clone();
    let output = match matches.get_one::<PathBuf>("output") {
        Some(path) => path.clone(),
        None => default_output(&dir),
    };
    let dry_run = matches.get_flag("dry_run");

    // Build exactly once, so --dry-run reports the same archive it would
    // write (timestamps/xattrs cannot change in between).
    let plan = maczip::build_maczip(&dir)?;
    if dry_run {
        for name in &plan.names {
            println!("would add {name}");
        }
        println!("would write {}", output.display());
        return Ok(());
    }

    maczip::write_plan(&plan, &output)?;
    println!("🗜️  wrote {}", output.display());
    for name in &plan.names {
        println!("   {name}");
    }
    Ok(())
}

fn default_output(dir: &Path) -> PathBuf {
    let parent = dir.parent().unwrap_or_else(|| Path::new("."));
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    parent.join(format!("{name}.zip"))
}
