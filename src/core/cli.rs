// SPDX-License-Identifier: Apache-2.0

//! Application core: feature-independent CLI bootstrap and dispatch.
//!
//! Each feature owns its [`clap::Command`] definition and execution under
//! `src/features/<feature>/cli.rs`.  This module only composes the
//! feature-gated root command and dispatches parsed matches.

use std::ffi::{OsStr, OsString};

use clap::error::ErrorKind;
use clap::Command;

use crate::shared::util::{Error, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(args: &[OsString]) -> Result<()> {
    // Preserve the two pre-clap conveniences that are not flag-shaped.
    if args.first().is_some_and(|arg| arg == OsStr::new("help")) {
        root_command().print_help()?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == OsStr::new("version")) {
        println!("mosbsfol {VERSION}");
        return Ok(());
    }

    // clap expects argv[0] to be the binary name; `args` contains argv[1..].
    let argv = std::iter::once(OsString::from("mosbsfol")).chain(args.iter().cloned());
    match root_command().try_get_matches_from(argv) {
        Ok(matches) => {
            let Some((name, sub_matches)) = matches.subcommand() else {
                root_command().print_help()?;
                return Ok(());
            };
            dispatch(name, sub_matches)
        }
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::DisplayVersion => {
                err.print()?;
                Ok(())
            }
            ErrorKind::InvalidSubcommand => Err(Error::new(format!(
                "{}
Run `mosbsfol --help` for usage.",
                render_clap_error(&err)
            ))),
            _ => Err(Error::new(render_clap_error(&err))),
        },
    }
}

#[allow(unused_variables)]
fn dispatch(name: &str, matches: &clap::ArgMatches) -> Result<()> {
    match name {
        #[cfg(feature = "autopoop")]
        "autopoop" | "daemon" => crate::features::autopoop::cli::execute(matches),
        #[cfg(feature = "dsstore")]
        "dsstore" => crate::features::dsstore::cli::execute(matches),
        #[cfg(feature = "dsstore")]
        "poop" => crate::features::dsstore::cli::execute_poop(matches),
        #[cfg(all(feature = "appledouble", feature = "dsstore"))]
        "usb" => crate::features::appledouble::cli::execute(matches),
        #[cfg(feature = "maczip")]
        "maczip" | "zip" => crate::features::maczip::cli::execute(matches),
        #[cfg(feature = "plist")]
        "plist" => crate::features::plist::cli::execute(matches),
        #[cfg(feature = "xattr")]
        "xattr" => crate::features::xattr::cli::execute(matches),
        #[cfg(feature = "volumetrace")]
        "trace" | "volumetrace" => crate::features::volumetrace::cli::execute(matches),
        other => Err(Error::new(format!("unknown command {other:?}"))),
    }
}

fn render_clap_error(err: &clap::Error) -> String {
    let rendered = err.to_string();
    rendered
        .strip_prefix("error: ")
        .unwrap_or(&rendered)
        .trim_end()
        .to_string()
}

#[allow(unused_mut)]
fn root_command() -> Command {
    let mut cmd = Command::new("mosbsfol")
        .version(VERSION)
        .about("MOSBSFOL - macOS Bull Shit Feature On Linux")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .disable_help_subcommand(true);

    #[cfg(feature = "autopoop")]
    {
        cmd = cmd.subcommand(crate::features::autopoop::cli::command());
    }

    #[cfg(feature = "dsstore")]
    {
        cmd = cmd
            .subcommand(crate::features::dsstore::cli::command())
            .subcommand(crate::features::dsstore::cli::shortcut_command());
    }

    #[cfg(all(feature = "appledouble", feature = "dsstore"))]
    {
        cmd = cmd.subcommand(crate::features::appledouble::cli::command());
    }

    #[cfg(feature = "maczip")]
    {
        cmd = cmd.subcommand(crate::features::maczip::cli::command());
    }

    #[cfg(feature = "plist")]
    {
        cmd = cmd.subcommand(crate::features::plist::cli::command());
    }

    #[cfg(feature = "xattr")]
    {
        cmd = cmd.subcommand(crate::features::xattr::cli::command());
    }

    #[cfg(feature = "volumetrace")]
    {
        cmd = cmd.subcommand(crate::features::volumetrace::cli::command());
    }

    cmd
}
