// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `plist` feature.

use std::path::PathBuf;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};

use super as plist;
use crate::shared::bplist;
use crate::shared::util::Result;

pub fn command() -> Command {
    Command::new("plist")
        .about("Read and write XML or binary property lists")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(write_command())
        .subcommand(read_command())
}

fn write_command() -> Command {
    Command::new("write")
        .about("Write a property list from key=value arguments")
        .alias("create")
        .arg(
            Arg::new("file")
                .value_parser(value_parser!(PathBuf))
                .required(true)
                .value_name("FILE")
                .help("Output plist path"),
        )
        .arg(
            Arg::new("key_values")
                .value_parser(value_parser!(String))
                .value_name("key=value")
                .num_args(0..)
                .action(ArgAction::Append)
                .help("Dictionary entries: true/false, integer, 0xHEX, 1.5, @base64:..., @hex:..."),
        )
        .arg(
            Arg::new("xml")
                .long("xml")
                .action(ArgAction::SetTrue)
                .help("Write XML instead of binary bplist00"),
        )
}

fn read_command() -> Command {
    Command::new("read")
        .about("Read and display a property list")
        .alias("cat")
        .arg(
            Arg::new("file")
                .value_parser(value_parser!(PathBuf))
                .required(true)
                .value_name("FILE")
                .help("Plist path to read"),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("write" | "create", matches)) => {
            let file = matches
                .get_one::<PathBuf>("file")
                .expect("clap requires FILE");
            let xml = matches.get_flag("xml");
            let key_values: Vec<String> = matches
                .get_many::<String>("key_values")
                .map(|values| values.cloned().collect())
                .unwrap_or_default();
            let value = plist::dict_from_args(&key_values)?;
            plist::write_file(file, &value, !xml)?;
            println!("{}", file.display());
            println!("{}", bplist::to_json(&value));
            Ok(())
        }
        Some(("read" | "cat", matches)) => {
            let file = matches
                .get_one::<PathBuf>("file")
                .expect("clap requires FILE");
            println!("{}", bplist::to_json(&plist::read_file(file)?));
            Ok(())
        }
        Some((other, _)) => Err(crate::shared::util::Error::new(format!(
            "unknown plist subcommand {other:?} (write|read)"
        ))),
        None => Err(crate::shared::util::Error::new(
            "usage: mosbsfol plist <write|read> ...",
        )),
    }
}
