// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `volumetrace` feature.

use clap::{ArgMatches, Command};

use super as volumetrace;
use crate::shared::cli::{dry_run_flag, optional_path, path_arg};
use crate::shared::util::Result;

pub fn command() -> Command {
    Command::new("trace")
        .about("Create or remove macOS volume-root droppings")
        .alias("volumetrace")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("poop")
                .about("Create .Spotlight-V100, .fseventsd, .Trashes, etc.")
                .alias("make")
                .arg(path_arg())
                .arg(dry_run_flag()),
        )
        .subcommand(
            Command::new("clean")
                .about("Remove macOS volume-root droppings")
                .arg(path_arg())
                .arg(dry_run_flag()),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    let Some((subcommand, matches)) = matches.subcommand() else {
        return Err(crate::shared::util::Error::new(
            "usage: mosbsfol trace <poop|clean> [PATH] [--dry-run]",
        ));
    };
    let path = optional_path(matches);
    let dry_run = matches.get_flag("dry_run");

    match subcommand {
        "poop" | "make" => {
            for created in volumetrace::poop_volume(&path, dry_run)? {
                println!("💩 created {}", created.display());
            }
            Ok(())
        }
        "clean" => {
            for removed in volumetrace::clean_volume(&path, dry_run)? {
                println!("removed {}", removed.display());
            }
            Ok(())
        }
        other => Err(crate::shared::util::Error::new(format!(
            "unknown trace subcommand {other:?} (poop|clean)"
        ))),
    }
}
