// SPDX-License-Identifier: Apache-2.0

//! Small [`clap`] argument builders shared by the feature commands.

use std::path::PathBuf;

use clap::{value_parser, Arg, ArgAction};

/// Optional `PATH` argument defaulting to the current directory at the
/// application level.
#[allow(dead_code)]
pub(crate) fn path_arg() -> Arg {
    Arg::new("path")
        .value_parser(value_parser!(PathBuf))
        .required(false)
        .value_name("PATH")
        .help("Target path (defaults to the current directory)")
}

#[allow(dead_code)]
pub(crate) fn recursive_flag() -> Arg {
    Arg::new("recursive")
        .short('r')
        .long("recursive")
        .action(ArgAction::SetTrue)
        .help("Recurse into subdirectories")
}

#[allow(dead_code)]
pub(crate) fn dry_run_flag() -> Arg {
    Arg::new("dry_run")
        .long("dry-run")
        .action(ArgAction::SetTrue)
        .help("Print what would be done without writing anything")
}

#[allow(dead_code)]
pub(crate) fn flag(id: &'static str, long: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(long)
        .action(ArgAction::SetTrue)
        .help(help)
}
