// SPDX-License-Identifier: Apache-2.0

//! Application core: feature-independent CLI bootstrap and dispatch.
//!
//! Each feature owns its command implementation and help text under
//! `src/features/<feature>/cli.rs`.  This module only routes argv and
//! composes the help screen from the features that were compiled in.

use crate::shared::util::{Error, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty()
        || args
            .iter()
            .any(|a| a == "-h" || a == "--help" || a == "help")
    {
        print_help();
        return Ok(());
    }
    if args[0] == "--version" || args[0] == "version" {
        println!("mosbsfol {VERSION}");
        return Ok(());
    }

    match args[0].as_str() {
        #[cfg(feature = "dsstore")]
        "dsstore" => crate::features::dsstore::cli::run(&args[1..]),
        #[cfg(all(feature = "appledouble", feature = "dsstore"))]
        "usb" => crate::features::appledouble::cli::run(&args[1..]),
        #[cfg(feature = "maczip")]
        "maczip" | "zip" => crate::features::maczip::cli::run(&args[1..]),
        #[cfg(feature = "plist")]
        "plist" => crate::features::plist::cli::run(&args[1..]),
        #[cfg(feature = "xattr")]
        "xattr" => crate::features::xattr::cli::run(&args[1..]),
        #[cfg(feature = "volumetrace")]
        "trace" | "volumetrace" => crate::features::volumetrace::cli::run(&args[1..]),
        #[cfg(feature = "dsstore")]
        "poop" => {
            let mut sub = vec!["poop".to_string()];
            sub.extend_from_slice(&args[1..]);
            crate::features::dsstore::cli::run(&sub)
        }
        other => Err(Error::new(format!(
            "unknown command {other:?}; run `mosbsfol --help`"
        ))),
    }
}

fn print_help() {
    println!("MOSBSFOL {VERSION} - macOS Bull Shit Feature On Linux");

    #[cfg(feature = "dsstore")]
    {
        print!("{}", crate::features::dsstore::cli::HELP);
        println!();
    }

    #[cfg(all(feature = "appledouble", feature = "dsstore"))]
    {
        print!("{}", crate::features::appledouble::cli::HELP);
        println!();
    }

    #[cfg(feature = "maczip")]
    {
        print!("{}", crate::features::maczip::cli::HELP);
        println!();
    }

    #[cfg(feature = "plist")]
    {
        print!("{}", crate::features::plist::cli::HELP);
        println!();
    }

    #[cfg(feature = "xattr")]
    {
        print!("{}", crate::features::xattr::cli::HELP);
        println!();
    }

    #[cfg(feature = "volumetrace")]
    {
        print!("{}", crate::features::volumetrace::cli::HELP);
        println!();
    }

    #[cfg(feature = "dsstore")]
    println!("\nShortcuts: `mosbsfol poop PATH` is `dsstore poop PATH`.");
}
