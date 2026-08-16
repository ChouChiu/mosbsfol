// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `xattr` feature.

use std::path::PathBuf;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};

use super as xattr;
use crate::shared::cli::required_path;
use crate::shared::util::{decode_hex, parse_yes_no, Error, Result};

pub fn command() -> Command {
    Command::new("xattr")
        .about("Read and write macOS-style extended attributes")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("list")
                .about("List xattrs on FILE")
                .arg(file_arg()),
        )
        .subcommand(
            Command::new("get")
                .about("Read one xattr")
                .arg(file_arg())
                .arg(name_arg()),
        )
        .subcommand(
            Command::new("set")
                .about("Write one xattr")
                .arg(file_arg())
                .arg(name_arg())
                .arg(value_arg())
                .arg(hex_flag()),
        )
        .subcommand(
            Command::new("del")
                .about("Remove one xattr")
                .alias("rm")
                .arg(file_arg())
                .arg(name_arg()),
        )
        .subcommand(
            Command::new("quarantine")
                .about("Set com.apple.quarantine")
                .arg(file_arg()),
        )
        .subcommand(
            Command::new("finderinfo")
                .about("Set the 32-byte com.apple.FinderInfo value")
                .arg(file_arg())
                .arg(
                    Arg::new("type_code")
                        .required(false)
                        .value_name("TYPE")
                        .help("Four-character type code (default: ????)"),
                )
                .arg(
                    Arg::new("creator_code")
                        .required(false)
                        .value_name("CREATOR")
                        .help("Four-character creator code (default: MACS)"),
                ),
        )
        .subcommand(
            Command::new("wherefroms")
                .about("Set com.apple.metadata:kMDItemWhereFroms")
                .arg(file_arg())
                .arg(
                    Arg::new("urls")
                        .value_parser(value_parser!(String))
                        .required(true)
                        .value_name("URL")
                        .num_args(1..)
                        .action(ArgAction::Append)
                        .help("One or more source URLs"),
                ),
        )
        .subcommand(
            Command::new("comment")
                .about("Set com.apple.metadata:kMDItemFinderComment")
                .arg(file_arg())
                .arg(
                    Arg::new("text")
                        .required(false)
                        .default_value("")
                        .value_name("TEXT")
                        .help("Comment text (default: empty)"),
                ),
        )
        .subcommand(
            Command::new("tag")
                .about("Set or read the Finder colour tag")
                .arg(file_arg())
                .arg(
                    Arg::new("color")
                        .required(false)
                        .value_name("COLOR")
                        .help("none/gray/green/purple/blue/yellow/red/orange"),
                ),
        )
        .subcommand(
            Command::new("hide")
                .about("Set or read the Finder hidden flag")
                .arg(file_arg())
                .arg(
                    Arg::new("value")
                        .required(false)
                        .value_name("YES|NO")
                        .help("yes/no/true/false/on/off/1/0"),
                ),
        )
        .subcommand(
            Command::new("resourcefork")
                .about("Set or read com.apple.ResourceFork")
                .arg(file_arg())
                .arg(
                    Arg::new("hex")
                        .required(false)
                        .value_name("HEX")
                        .help("Hex-encoded resource fork bytes"),
                ),
        )
}

fn file_arg() -> Arg {
    Arg::new("file")
        .value_parser(value_parser!(PathBuf))
        .required(true)
        .value_name("FILE")
        .help("Target file or directory")
}

fn name_arg() -> Arg {
    Arg::new("name")
        .required(true)
        .value_name("NAME")
        .help("Extended attribute name")
}

fn value_arg() -> Arg {
    Arg::new("value")
        .required(true)
        .value_name("VALUE")
        .help("Raw value, or hex bytes with --hex")
}

fn hex_flag() -> Arg {
    Arg::new("hex")
        .long("hex")
        .action(ArgAction::SetTrue)
        .help("Treat VALUE as hex-encoded bytes")
}

pub fn execute(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("list", matches)) => execute_list(matches),
        Some(("get", matches)) => execute_get(matches),
        Some(("set", matches)) => execute_set(matches),
        Some(("del" | "rm", matches)) => execute_del(matches),
        Some(("quarantine", matches)) => execute_quarantine(matches),
        Some(("finderinfo", matches)) => execute_finderinfo(matches),
        Some(("wherefroms", matches)) => execute_wherefroms(matches),
        Some(("comment", matches)) => execute_comment(matches),
        Some(("tag", matches)) => execute_tag(matches),
        Some(("hide", matches)) => execute_hide(matches),
        Some(("resourcefork", matches)) => execute_resourcefork(matches),
        Some((other, _)) => Err(Error::new(format!("unknown xattr subcommand {other:?}"))),
        None => Err(Error::new(
            "usage: mosbsfol xattr <list|get|set|del|quarantine|finderinfo|wherefroms|comment|tag|hide|resourcefork> ...",
        )),
    }
}

fn execute_list(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let names = xattr::list(path)?;
    for name in names {
        let shown = xattr::display_name(&name);
        match xattr::get(path, &name) {
            Ok(raw) => println!("{shown}: {}", xattr::display_value(&shown, &raw)),
            Err(error) => println!("{shown}: <{error}>"),
        }
    }
    Ok(())
}

fn execute_get(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let name = matches
        .get_one::<String>("name")
        .expect("clap requires NAME");
    let raw = xattr::get(path, name)?;
    println!("{}", xattr::display_value(name, &raw));
    Ok(())
}

fn execute_set(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let name = matches
        .get_one::<String>("name")
        .expect("clap requires NAME");
    let value = matches
        .get_one::<String>("value")
        .expect("clap requires VALUE");
    let raw = if matches.get_flag("hex") {
        decode_hex(value, "hex value")?
    } else {
        value.clone().into_bytes()
    };
    xattr::set(path, name, &raw)
}

fn execute_del(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let name = matches
        .get_one::<String>("name")
        .expect("clap requires NAME");
    xattr::remove(path, name)
}

fn execute_quarantine(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    xattr::set_quarantine(path)?;
    let raw = xattr::get(path, "com.apple.quarantine")?;
    println!("com.apple.quarantine: {}", String::from_utf8_lossy(&raw));
    Ok(())
}

fn execute_finderinfo(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let type_code = matches
        .get_one::<String>("type_code")
        .map(|value| fourcc_arg(value))
        .transpose()?
        .unwrap_or(*b"????");
    let creator_code = matches
        .get_one::<String>("creator_code")
        .map(|value| fourcc_arg(value))
        .transpose()?
        .unwrap_or(*b"MACS");
    xattr::set_finder_info(path, &type_code, &creator_code)
}

fn execute_wherefroms(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let urls: Vec<String> = matches
        .get_many::<String>("urls")
        .map(|values| values.cloned().collect())
        .unwrap_or_default();
    xattr::set_where_froms(path, &urls)?;
    let raw = xattr::get(path, "com.apple.metadata:kMDItemWhereFroms")?;
    println!(
        "{}",
        xattr::display_value("com.apple.metadata:kMDItemWhereFroms", &raw)
    );
    Ok(())
}

fn execute_comment(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    let text = matches
        .get_one::<String>("text")
        .map(String::as_str)
        .unwrap_or("");
    xattr::set_finder_comment(path, text)
}

fn execute_tag(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    if let Some(color) = matches.get_one::<String>("color") {
        xattr::set_finder_tag(path, color)?;
    }
    println!(
        "tag: {}",
        xattr::finder_tag_name(xattr::get_finder_tag(path)?)
    );
    Ok(())
}

fn execute_hide(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    if let Some(value) = matches.get_one::<String>("value") {
        xattr::set_hidden(path, parse_yes_no(value)?)?;
    }
    println!("hidden: {}", xattr::is_hidden(path)?);
    Ok(())
}

fn execute_resourcefork(matches: &ArgMatches) -> Result<()> {
    let path = required_path(matches, "file");
    if let Some(hex) = matches.get_one::<String>("hex") {
        let data = decode_hex(hex, "hex resource fork")?;
        xattr::set_resource_fork(path, &data)?;
    }
    let raw = xattr::get_resource_fork(path)?;
    println!(
        "com.apple.ResourceFork: {}",
        crate::shared::util::hex_dump(&raw, 64)
    );
    Ok(())
}

fn fourcc_arg(value: &str) -> Result<[u8; 4]> {
    let bytes = value.as_bytes();
    if bytes.len() != 4 || !bytes.is_ascii() {
        return Err(Error::new(format!(
            "type/creator must be a four-character ASCII code, got {value:?}"
        )));
    }
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}
