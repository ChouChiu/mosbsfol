// SPDX-License-Identifier: Apache-2.0

//! CLI command owned by the `xattr` feature.

use std::path::Path;

use super as xattr;
use crate::shared::cli::{has_flag, need};
use crate::shared::util::{Error, Result};

pub const HELP: &str = r#"
    xattr         xattr list FILE
                  xattr get FILE NAME
                  xattr set FILE NAME VALUE [--hex]
                  xattr del FILE NAME
                  xattr quarantine FILE
                  xattr finderinfo FILE [TYPE CREATOR]
                  xattr wherefroms FILE URL...
                  xattr comment FILE TEXT
                  xattr tag FILE [COLOR|none]
                  xattr hide FILE [yes|no]
                  xattr resourcefork FILE [HEX]"#;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new(
            "usage: mosbsfol xattr <list|get|set|del|quarantine|finderinfo|wherefroms|comment|tag|hide|resourcefork> ...",
        ));
    }
    match args[0].as_str() {
        "list" => {
            let path = need(&args[1..], 0, "file path")?;
            let names = xattr::list(Path::new(&path))?;
            for name in names {
                let shown = xattr::display_name(&name);
                match xattr::get(Path::new(&path), &name) {
                    Ok(raw) => println!("{shown}: {}", xattr::display_value(&shown, &raw)),
                    Err(e) => println!("{shown}: <{e}>"),
                }
            }
            Ok(())
        }
        "get" => {
            let path = need(&args[1..], 0, "file path")?;
            let name = need(&args[1..], 1, "attribute name")?;
            let raw = xattr::get(Path::new(&path), &name)?;
            println!("{}", xattr::display_value(&name, &raw));
            Ok(())
        }
        "set" => {
            let path = need(&args[1..], 0, "file path")?;
            let name = need(&args[1..], 1, "attribute name")?;
            let value = need(&args[1..], 2, "attribute value")?;
            let raw = if has_flag(&args[1..], &["--hex"]) {
                let mut out = Vec::new();
                if value.len() % 2 != 0 {
                    return Err(Error::new("odd-length hex value"));
                }
                for i in (0..value.len()).step_by(2) {
                    out.push(
                        u8::from_str_radix(&value[i..i + 2], 16)
                            .map_err(|_| Error::new("invalid hex value"))?,
                    );
                }
                out
            } else {
                value.into_bytes()
            };
            xattr::set(Path::new(&path), &name, &raw)?;
            Ok(())
        }
        "del" | "rm" => {
            let path = need(&args[1..], 0, "file path")?;
            let name = need(&args[1..], 1, "attribute name")?;
            xattr::remove(Path::new(&path), &name)?;
            Ok(())
        }
        "quarantine" => {
            let path = need(&args[1..], 0, "file path")?;
            xattr::set_quarantine(Path::new(&path))?;
            let raw = xattr::get(Path::new(&path), "com.apple.quarantine")?;
            println!("com.apple.quarantine: {}", String::from_utf8_lossy(&raw));
            Ok(())
        }
        "finderinfo" => {
            let path = need(&args[1..], 0, "file path")?;
            let type_code = args
                .get(2)
                .map(|s| fourcc_arg(s))
                .transpose()?
                .unwrap_or(*b"????");
            let creator = args
                .get(3)
                .map(|s| fourcc_arg(s))
                .transpose()?
                .unwrap_or(*b"MACS");
            xattr::set_finder_info(Path::new(&path), &type_code, &creator)?;
            Ok(())
        }
        "wherefroms" => {
            let path = need(&args[1..], 0, "file path")?;
            if args.len() < 3 {
                return Err(Error::new("usage: mosbsfol xattr wherefroms FILE URL..."));
            }
            let urls: Vec<String> = args[2..].to_vec();
            xattr::set_where_froms(Path::new(&path), &urls)?;
            let raw = xattr::get(Path::new(&path), "com.apple.metadata:kMDItemWhereFroms")?;
            println!(
                "{}",
                xattr::display_value("com.apple.metadata:kMDItemWhereFroms", &raw)
            );
            Ok(())
        }
        "comment" => {
            let path = need(&args[1..], 0, "file path")?;
            let text = args.get(2).map(|s| s.as_str()).unwrap_or("");
            xattr::set_finder_comment(Path::new(&path), text)?;
            Ok(())
        }
        "tag" => {
            let path = need(&args[1..], 0, "file path")?;
            if let Some(color) = args.get(2) {
                xattr::set_finder_tag(Path::new(&path), color)?;
            }
            println!(
                "tag: {}",
                xattr::finder_tag_name(xattr::get_finder_tag(Path::new(&path)))
            );
            Ok(())
        }
        "hide" => {
            let path = need(&args[1..], 0, "file path")?;
            if let Some(value) = args.get(2) {
                let hidden = match value.to_ascii_lowercase().as_str() {
                    "yes" | "true" | "on" | "1" => true,
                    "no" | "false" | "off" | "0" => false,
                    other => return Err(Error::new(format!("expected yes/no, got {other:?}"))),
                };
                xattr::set_hidden(Path::new(&path), hidden)?;
            }
            println!("hidden: {}", xattr::is_hidden(Path::new(&path)));
            Ok(())
        }
        "resourcefork" => {
            let path = need(&args[1..], 0, "file path")?;
            if let Some(hex) = args.get(2) {
                if !hex.len().is_multiple_of(2) {
                    return Err(Error::new("odd-length hex resource fork"));
                }
                let mut data = Vec::with_capacity(hex.len() / 2);
                for i in (0..hex.len()).step_by(2) {
                    data.push(
                        u8::from_str_radix(&hex[i..i + 2], 16)
                            .map_err(|_| Error::new("invalid hex resource fork"))?,
                    );
                }
                xattr::set_resource_fork(Path::new(&path), &data)?;
            }
            let raw = xattr::get_resource_fork(Path::new(&path))?;
            println!(
                "com.apple.ResourceFork: {}",
                crate::shared::util::hex_dump(&raw, 64)
            );
            Ok(())
        }
        other => Err(Error::new(format!("unknown xattr subcommand {other:?}"))),
    }
}

fn fourcc_arg(s: &str) -> Result<[u8; 4]> {
    let b = s.as_bytes();
    if b.len() != 4 {
        return Err(Error::new(format!(
            "type/creator must be a four-character code, got {s:?}"
        )));
    }
    Ok([b[0], b[1], b[2], b[3]])
}
