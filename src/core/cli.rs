// SPDX-License-Identifier: Apache-2.0

//! Application core: feature-independent CLI bootstrap and dispatch.
//!
//! Each feature owns its [`clap::Command`] definition and execution under
//! `src/features/<feature>/cli.rs`.  This module only composes the
//! feature-gated root command and dispatches parsed matches.

use anyhow::{anyhow, Result};
use clap::error::ErrorKind;
use clap::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[allow(unused_variables)]
pub fn run(args: &[String]) -> Result<()> {
    // Preserve the two pre-clap conveniences that are not flag-shaped.
    if args.first().is_some_and(|a| a == "help") {
        root_command().print_help()?;
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "version") {
        println!("mosbsfol {VERSION}");
        return Ok(());
    }

    // clap expects argv[0] to be the binary name; `args` contains argv[1..].
    let argv = std::iter::once("mosbsfol").chain(args.iter().map(String::as_str));
    match root_command().try_get_matches_from(argv) {
        Ok(matches) => {
            let Some((name, sub_matches)) = matches.subcommand() else {
                root_command().print_help()?;
                return Ok(());
            };
            match name {
                #[cfg(feature = "dsstore")]
                "dsstore" => Ok(crate::features::dsstore::cli::execute(sub_matches)?),
                #[cfg(feature = "dsstore")]
                "poop" => Ok(crate::features::dsstore::cli::execute_poop(sub_matches)?),
                #[cfg(all(feature = "appledouble", feature = "dsstore"))]
                "usb" => Ok(crate::features::appledouble::cli::execute(sub_matches)?),
                #[cfg(feature = "maczip")]
                "maczip" | "zip" => Ok(crate::features::maczip::cli::execute(sub_matches)?),
                #[cfg(feature = "plist")]
                "plist" => Ok(crate::features::plist::cli::execute(sub_matches)?),
                #[cfg(feature = "xattr")]
                "xattr" => Ok(crate::features::xattr::cli::execute(sub_matches)?),
                #[cfg(feature = "volumetrace")]
                "trace" | "volumetrace" => {
                    Ok(crate::features::volumetrace::cli::execute(sub_matches)?)
                }
                other => Err(anyhow!("unknown command {other:?}")),
            }
        }
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::DisplayVersion => {
                err.print()?;
                Ok(())
            }
            ErrorKind::InvalidSubcommand => Err(anyhow!(
                "{}
Run `mosbsfol --help` for usage.",
                render_clap_error(&err)
            )),
            _ => Err(anyhow!(render_clap_error(&err))),
        },
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
