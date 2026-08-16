// SPDX-License-Identifier: Apache-2.0

//! `usb` command: composes the `appledouble` and `dsstore` features, so
//! this module is compiled only when both are enabled.

use std::path::PathBuf;

use clap::{ArgMatches, Command};

use super as appledouble;
use crate::features::dsstore::finder::{self, FinderOptions};
use crate::shared::cli::{dry_run_flag, flag, path_arg, recursive_flag};
use crate::shared::util::Result;

pub fn command() -> Command {
    Command::new("usb")
        .about("Recreate the droppings macOS leaves on a FAT/exFAT USB stick")
        .arg(path_arg())
        .arg(recursive_flag())
        .arg(flag(
            "include_dirs",
            "include-dirs",
            "Also create AppleDouble sidecars for directories",
        ))
        .arg(flag(
            "type_codes",
            "type-codes",
            "Use plausible macOS type codes instead of '????'",
        ))
        .arg(dry_run_flag())
        .arg(flag(
            "clean",
            "clean",
            "Remove ._* sidecars, .DS_Store files and volume traces",
        ))
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    let path = matches
        .get_one::<PathBuf>("path")
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let recursive = matches.get_flag("recursive");
    let dry_run = matches.get_flag("dry_run");

    if matches.get_flag("clean") {
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

    let include_dirs = matches.get_flag("include_dirs");
    let type_codes = matches.get_flag("type_codes");

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
