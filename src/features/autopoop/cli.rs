// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `autopoop` feature.

use std::path::PathBuf;

use clap::{value_parser, Arg, ArgMatches, Command};

use super::{
    disable, enable, is_enabled, run_daemon, run_local_once, run_once, run_trigger, state_path,
    AutopoopOptions,
};
use crate::shared::cli::{dry_run_flag, recursive_flag};
use crate::shared::util::{Error, Result};

pub fn command() -> Command {
    Command::new("autopoop")
        .visible_alias("daemon")
        .about("Automatically drop macOS droppings on removable media and the local machine")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(run_command())
        .subcommand(once_command())
        .subcommand(local_command())
        .subcommand(trigger_command())
        .subcommand(enable_command())
        .subcommand(disable_command())
        .subcommand(status_command())
}

fn run_command() -> Command {
    Command::new("run")
        .about("Run the autopoop watcher daemon in the foreground")
        .arg(interval_arg())
        .arg(local_rescan_arg())
        .arg(no_local_flag())
        .arg(local_recursive_flag())
        .arg(state_arg())
        .arg(dry_run_flag())
}

fn once_command() -> Command {
    Command::new("once")
        .about("Poop mounted removable media and local disks once, then exit")
        .arg(
            Arg::new("path")
                .value_parser(value_parser!(PathBuf))
                .required(false)
                .value_name("PATH")
                .help("Poop this directory with the full USB suite instead of scanning mounts"),
        )
        .arg(no_local_flag())
        .arg(local_recursive_flag())
        .arg(force_flag())
        .arg(state_arg())
        .arg(dry_run_flag())
}

fn local_command() -> Command {
    Command::new("local")
        .about("Poop the local machine like a Mac HFS volume: .DS_Store + volume traces")
        .arg(
            Arg::new("path")
                .value_parser(value_parser!(PathBuf))
                .required(false)
                .value_name("PATH")
                .help("Local directory to poop (default: every mounted local fixed disk)"),
        )
        .arg(recursive_flag())
        .arg(force_flag())
        .arg(state_arg())
        .arg(dry_run_flag())
}

fn trigger_command() -> Command {
    Command::new("trigger")
        .about("Handle one udev block add event (called by the udev rule)")
        .arg(
            Arg::new("device")
                .required(true)
                .value_name("MAJ:MIN|DEVNODE")
                .help("Block device as MAJ:MIN (udev %M:%m) or /dev/sdb1"),
        )
        .arg(include_local_flag())
        .arg(local_recursive_flag())
        .arg(force_flag())
        .arg(state_arg())
        .arg(dry_run_flag())
}

fn enable_command() -> Command {
    Command::new("enable")
        .about("Switch automatic pooping on (removable media + local machine)")
        .alias("on")
        .arg(state_arg())
}

fn disable_command() -> Command {
    Command::new("disable")
        .about("Switch automatic pooping off")
        .alias("off")
        .arg(state_arg())
}

fn status_command() -> Command {
    Command::new("status")
        .about("Show whether automatic pooping is switched on")
        .arg(state_arg())
}

fn interval_arg() -> Arg {
    Arg::new("interval")
        .long("interval")
        .value_name("SECONDS")
        .value_parser(value_parser!(u64).range(1..))
        .default_value("2")
        .help("Polling interval for new removable/local mounts")
}

fn local_rescan_arg() -> Arg {
    Arg::new("local_rescan")
        .long("local-rescan")
        .value_name("SECONDS")
        .value_parser(value_parser!(u64).range(1..))
        .default_value("3600")
        .help("How often already-mounted local disk roots are re-pooped")
}

fn state_arg() -> Arg {
    Arg::new("state")
        .long("state")
        .value_name("FILE")
        .value_parser(value_parser!(PathBuf))
        .help(
            "Switch state file (default: $MOSBSFOL_AUTOPOOP_STATE or /run/mosbsfol/autopoop/state)",
        )
}

fn force_flag() -> Arg {
    Arg::new("force")
        .long("force")
        .action(clap::ArgAction::SetTrue)
        .help("Poop even while the autopoop switch is disabled")
}

fn no_local_flag() -> Arg {
    Arg::new("no_local")
        .long("no-local")
        .action(clap::ArgAction::SetTrue)
        .help("Leave local fixed disks alone; only watch removable media")
}

fn local_recursive_flag() -> Arg {
    Arg::new("local_recursive")
        .long("local-recursive")
        .action(clap::ArgAction::SetTrue)
        .help("Recurse into local mount roots when creating .DS_Store files")
}

fn include_local_flag() -> Arg {
    Arg::new("include_local")
        .long("include-local")
        .action(clap::ArgAction::SetTrue)
        .help("Also handle local fixed disks in a udev trigger")
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("run", matches)) => execute_run(matches),
        Some(("once", matches)) => execute_once(matches),
        Some(("local", matches)) => execute_local(matches),
        Some(("trigger", matches)) => execute_trigger(matches),
        Some(("enable" | "on", matches)) => execute_enable(matches),
        Some(("disable" | "off", matches)) => execute_disable(matches),
        Some(("status", matches)) => execute_status(matches),
        Some((other, _)) => Err(Error::new(format!(
            "unknown autopoop subcommand {other:?} (run|once|local|trigger|enable|disable|status)"
        ))),
        None => Err(Error::new(
            "usage: mosbsfol autopoop <run|once|local|trigger|enable|disable|status> ...",
        )),
    }
}

fn execute_run(matches: &ArgMatches) -> Result<()> {
    let interval = *matches
        .get_one::<u64>("interval")
        .expect("clap has a default interval");
    let dry_run = matches.get_flag("dry_run");
    run_daemon(
        interval,
        &state_path(state(matches)),
        dry_run,
        autopoop_options(matches),
    )
}

fn execute_once(matches: &ArgMatches) -> Result<()> {
    let path = matches.get_one::<PathBuf>("path");
    let force = matches.get_flag("force");
    let dry_run = matches.get_flag("dry_run");
    run_once(
        path.map(PathBuf::as_path),
        force,
        &state_path(state(matches)),
        dry_run,
        autopoop_options(matches),
    )
}

fn execute_local(matches: &ArgMatches) -> Result<()> {
    let path = matches.get_one::<PathBuf>("path");
    let recursive = matches.get_flag("recursive");
    let force = matches.get_flag("force");
    let dry_run = matches.get_flag("dry_run");
    run_local_once(
        path.map(PathBuf::as_path),
        recursive,
        force,
        &state_path(state(matches)),
        dry_run,
    )
}

fn execute_trigger(matches: &ArgMatches) -> Result<()> {
    let device = matches
        .get_one::<String>("device")
        .expect("clap requires DEVICE");
    let force = matches.get_flag("force");
    let dry_run = matches.get_flag("dry_run");
    run_trigger(
        device,
        force,
        &state_path(state(matches)),
        dry_run,
        matches.get_flag("include_local"),
        matches.get_flag("local_recursive"),
    )
}

fn execute_enable(matches: &ArgMatches) -> Result<()> {
    let state = state_path(state(matches));
    enable(&state)?;
    println!(
        "🚽 autopoop enabled: removable media and the local machine will be pooped automatically (state {})",
        state.display()
    );
    Ok(())
}

fn execute_disable(matches: &ArgMatches) -> Result<()> {
    let state = state_path(state(matches));
    disable(&state)?;
    println!(
        "🧻 autopoop disabled: removable media and the local machine will be left alone (state {})",
        state.display()
    );
    Ok(())
}

fn execute_status(matches: &ArgMatches) -> Result<()> {
    let state = state_path(state(matches));
    let enabled = is_enabled(&state)?;
    println!(
        "autopoop is {} (state {})",
        if enabled { "ENABLED" } else { "DISABLED" },
        state.display()
    );
    Ok(())
}

fn autopoop_options(matches: &ArgMatches) -> AutopoopOptions {
    AutopoopOptions {
        include_local: !matches.get_flag("no_local"),
        local_recursive: matches.get_flag("local_recursive"),
        local_rescan_secs: matches
            .try_get_one::<u64>("local_rescan")
            .ok()
            .flatten()
            .copied()
            .unwrap_or(3600),
    }
}

fn state(matches: &ArgMatches) -> Option<&std::path::Path> {
    matches.get_one::<PathBuf>("state").map(PathBuf::as_path)
}
