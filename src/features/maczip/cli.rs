// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `maczip` feature.

use std::path::{Path, PathBuf};

use super as maczip;
use crate::shared::cli::{first_positional, has_flag};
use crate::shared::util::{Error, Result};

pub const HELP: &str = r#"
    maczip        maczip DIR [OUT.zip] [--dry-run]"#;

pub fn run(args: &[String]) -> Result<()> {
    let dir = PathBuf::from(first_positional(args, true).ok_or_else(|| Error::new("missing DIR"))?);
    let output = match first_positional(&args[1..], true) {
        Some(p) => PathBuf::from(p),
        None => {
            let parent = dir.parent().unwrap_or_else(|| Path::new("."));
            let name = dir.file_name().unwrap_or_default().to_string_lossy();
            parent.join(format!("{name}.zip"))
        }
    };
    let dry_run = has_flag(args, &["--dry-run"]);
    let (_, names) = maczip::build_maczip(&dir)?;
    if dry_run {
        for name in names {
            println!("would add {name}");
        }
        println!("would write {}", output.display());
        return Ok(());
    }
    let written = maczip::write_maczip(&dir, &output)?;
    println!("🗜️  wrote {}", output.display());
    for name in written {
        println!("   {name}");
    }
    Ok(())
}
