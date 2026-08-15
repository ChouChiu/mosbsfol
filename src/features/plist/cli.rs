// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `plist` feature.

use std::path::Path;

use super as plist;
use crate::shared::bplist;
use crate::shared::cli::{has_flag, need, positionals_after};
use crate::shared::util::{Error, Result};

pub const HELP: &str = r#"
    plist         plist write FILE [key=value ...] [--xml]
                  plist read FILE
                  value syntax: true/false, integer, 0xHEX, 1.5,
                                @base64:..., @hex:..."#;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new("usage: mosbsfol plist <write|read> ..."));
    }
    match args[0].as_str() {
        "write" | "create" => {
            let path = need(&args[1..], 0, "output plist path")?;
            let xml = has_flag(&args[1..], &["--xml"]);
            let kv: Vec<String> = positionals_after(&args[1..], 1);
            let value = plist::dict_from_args(&kv)?;
            plist::write_file(Path::new(&path), &value, !xml)?;
            println!("{}", path);
            println!("{}", bplist::to_json(&value));
            Ok(())
        }
        "read" | "cat" => {
            let path = need(&args[1..], 0, "plist path")?;
            let value = plist::read_file(Path::new(&path))?;
            println!("{}", bplist::to_json(&value));
            Ok(())
        }
        other => Err(Error::new(format!(
            "unknown plist subcommand {other:?} (write|read)"
        ))),
    }
}
