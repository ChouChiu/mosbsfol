// SPDX-License-Identifier: Apache-2.0

//! `usb` command: composes the `appledouble` and `dsstore` features, so
//! this module is compiled only when both are enabled.

use std::path::PathBuf;

use super as appledouble;
use crate::features::dsstore::finder::{self, FinderOptions};
use crate::shared::cli::{first_positional, has_flag};
use crate::shared::util::Result;

pub const HELP: &str = r#"
    USB droppings usb [PATH] [-r] [--include-dirs] [--type-codes]
                      [--dry-run] [--clean]"#;

pub fn run(args: &[String]) -> Result<()> {
    if has_flag(args, &["--help", "-h"]) {
        println!(
            "usage: mosbsfol usb [PATH] [-r] [--include-dirs] [--type-codes] [--dry-run] [--clean]"
        );
        return Ok(());
    }
    if has_flag(args, &["--clean"]) {
        let path = PathBuf::from(first_positional(args, true).unwrap_or_else(|| ".".into()));
        let recursive = has_flag(args, &["-r", "--recursive"]);
        let dry_run = has_flag(args, &["--dry-run"]);
        let sidecars = appledouble::clean_tree(&path, recursive, dry_run)?;
        let stores = finder::clean_tree(&path, recursive, dry_run)?;
        for f in sidecars.into_iter().chain(stores) {
            println!("removed {}", f.display());
        }
        #[cfg(feature = "volumetrace")]
        for f in crate::features::volumetrace::clean_volume(&path, dry_run)? {
            println!("removed {}", f.display());
        }
        return Ok(());
    }

    let path = PathBuf::from(first_positional(args, true).unwrap_or_else(|| ".".into()));
    let recursive = has_flag(args, &["-r", "--recursive"]);
    let include_dirs = has_flag(args, &["--include-dirs"]);
    let type_codes = has_flag(args, &["--type-codes"]);
    let dry_run = has_flag(args, &["--dry-run"]);

    let sidecars = appledouble::poop_tree(&path, recursive, include_dirs, type_codes, dry_run)?;
    let opts = FinderOptions::default();
    let stores = finder::poop_tree(&path, &opts, recursive, false, dry_run)?;
    for f in sidecars {
        println!("💩 sidecar {}", f.display());
    }
    for f in stores {
        println!("💩 .DS_Store {}", f.display());
    }
    #[cfg(feature = "volumetrace")]
    for f in crate::features::volumetrace::poop_volume(&path, dry_run)? {
        println!("💩 volume trace {}", f.display());
    }
    println!("🪰 USB stick now smells like a Mac.");
    Ok(())
}
