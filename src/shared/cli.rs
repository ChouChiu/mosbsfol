// SPDX-License-Identifier: Apache-2.0

//! Small [`clap`] argument builders shared by the feature commands.
#![allow(dead_code)] // each helper is only referenced by a subset of Cargo features

use std::path::{Path, PathBuf};

use clap::{value_parser, Arg, ArgAction, ArgMatches};

/// Optional `PATH` argument defaulting to the current directory at the
/// application level.
pub(crate) fn path_arg() -> Arg {
    Arg::new("path")
        .value_parser(value_parser!(PathBuf))
        .required(false)
        .value_name("PATH")
        .help("Target path (defaults to the current directory)")
}

pub(crate) fn recursive_flag() -> Arg {
    Arg::new("recursive")
        .short('r')
        .long("recursive")
        .action(ArgAction::SetTrue)
        .help("Recurse into subdirectories")
}

pub(crate) fn dry_run_flag() -> Arg {
    Arg::new("dry_run")
        .long("dry-run")
        .action(ArgAction::SetTrue)
        .help("Print what would be done without writing anything")
}

pub(crate) fn flag(id: &'static str, long: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(long)
        .action(ArgAction::SetTrue)
        .help(help)
}

/// Resolve the optional `path` argument to a concrete path.
pub(crate) fn optional_path(matches: &ArgMatches) -> PathBuf {
    matches
        .get_one::<PathBuf>("path")
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Borrow the required `file`/`dir` argument.
pub(crate) fn required_path<'a>(matches: &'a ArgMatches, id: &str) -> &'a Path {
    matches
        .get_one::<PathBuf>(id)
        .expect("clap requires the argument")
        .as_path()
}
