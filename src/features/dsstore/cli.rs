// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `dsstore` feature.

use std::path::PathBuf;

use clap::{value_parser, Arg, ArgMatches, Command};

use super::finder::{self, FinderOptions};
use super::format::{display_record, records_sorted_cmp, DsStore};
use crate::shared::cli::{dry_run_flag, flag, path_arg, recursive_flag};
use crate::shared::util::Result;

pub fn command() -> Command {
    Command::new("dsstore")
        .about("Generate, inspect and clean Finder .DS_Store files")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(poop_command())
        .subcommand(inspect_command())
        .subcommand(clean_command())
}

/// Top-level `mosbsfol poop PATH` shortcut for `mosbsfol dsstore poop`.
pub fn shortcut_command() -> Command {
    poop_args(Command::new("poop").about("Shortcut for `mosbsfol dsstore poop`"))
}

fn poop_command() -> Command {
    poop_args(
        Command::new("poop")
            .about("Create Finder-style .DS_Store files")
            .alias("make"),
    )
}

fn poop_args(command: Command) -> Command {
    command
        .arg(path_arg())
        .arg(recursive_flag())
        .arg(flag(
            "skip_hidden",
            "skip-hidden",
            "Skip dotfiles when describing the directory",
        ))
        .arg(dry_run_flag())
}

fn inspect_command() -> Command {
    Command::new("inspect")
        .about("Parse and display a .DS_Store file")
        .alias("read")
        .arg(
            Arg::new("file")
                .value_parser(value_parser!(PathBuf))
                .required(true)
                .value_name("FILE")
                .help(".DS_Store file to inspect"),
        )
}

fn clean_command() -> Command {
    Command::new("clean")
        .about("Remove .DS_Store files")
        .arg(path_arg())
        .arg(recursive_flag())
        .arg(dry_run_flag())
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("poop" | "make", matches)) => execute_poop(matches),
        Some(("inspect" | "read", matches)) => execute_inspect(matches),
        Some(("clean", matches)) => execute_clean(matches),
        Some((other, _)) => Err(crate::shared::util::Error::new(format!(
            "unknown dsstore subcommand {other:?} (poop|inspect|clean)"
        ))),
        None => Err(crate::shared::util::Error::new(
            "usage: mosbsfol dsstore <poop|inspect|clean> ...",
        )),
    }
}

pub fn execute_poop(matches: &ArgMatches) -> Result<()> {
    let path = optional_path(matches);
    let recursive = matches.get_flag("recursive");
    let skip_hidden = matches.get_flag("skip_hidden");
    let dry_run = matches.get_flag("dry_run");
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

fn execute_inspect(matches: &ArgMatches) -> Result<()> {
    let file = matches
        .get_one::<PathBuf>("file")
        .expect("clap requires FILE");
    let bytes = std::fs::read(file)?;
    let parsed = DsStore::parse(&bytes)?;
    println!(
        "{}: .DS_Store, {} records, allocator @0x{:x} size 0x{:x}, {} blocks",
        file.display(),
        parsed.record_count(),
        parsed.allocator_offset,
        parsed.allocator_size,
        parsed.block_count
    );
    let mut records = parsed.records.clone();
    records.sort_by(records_sorted_cmp);
    for record in records {
        println!("  {}", display_record(&record));
    }
    Ok(())
}

fn execute_clean(matches: &ArgMatches) -> Result<()> {
    let path = optional_path(matches);
    let recursive = matches.get_flag("recursive");
    let dry_run = matches.get_flag("dry_run");
    let files = finder::clean_tree(&path, recursive, dry_run)?;
    for f in &files {
        println!("removed {}", f.display());
    }
    if files.is_empty() {
        println!("no .DS_Store files found");
    }
    Ok(())
}

fn optional_path(matches: &ArgMatches) -> PathBuf {
    matches
        .get_one::<PathBuf>("path")
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}
