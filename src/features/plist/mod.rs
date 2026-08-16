// SPDX-License-Identifier: Apache-2.0

//! Property-list file helpers.
//!
//! Both the NeXT/Apple XML plist and the binary `bplist00` encoding are
//! handled by the maintained [`plist`] crate.

pub mod cli;

use std::fs;
use std::io::Cursor;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use crate::shared::bplist::{self, Dictionary, Plist};
use crate::shared::util::{Error, Result};

fn plist_error(e: plist::Error) -> Error {
    Error::new(format!("invalid plist: {e}"))
}

/// Parse data in either binary or XML form.
pub fn parse_auto(data: &[u8]) -> Result<Plist> {
    Plist::from_reader(Cursor::new(data)).map_err(plist_error)
}

/// Read a plist file in either representation.
pub fn read_file(path: &Path) -> Result<Plist> {
    let data = fs::read(path)?;
    parse_auto(&data)
}

/// Write a plist file.  Binary by default; XML with `binary == false`.
pub fn write_file(path: &Path, value: &Plist, binary: bool) -> Result<()> {
    let data = if binary {
        bplist::encode(value)?
    } else {
        let mut xml = Vec::new();
        value.to_writer_xml(&mut xml).map_err(plist_error)?;
        xml
    };
    fs::write(path, data)?;
    Ok(())
}

/// Infer a `Plist` value from a `key=value` argument.
pub fn value_from_arg(raw: &str) -> Result<(String, Plist)> {
    let Some((key, value)) = raw.split_once('=') else {
        return Err(Error::new(format!("expected key=value, got {raw:?}")));
    };
    if key.is_empty() {
        return Err(Error::new("empty plist key"));
    }
    let value = if value.eq_ignore_ascii_case("true") {
        Plist::Boolean(true)
    } else if value.eq_ignore_ascii_case("false") {
        Plist::Boolean(false)
    } else if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let n = i64::from_str_radix(hex, 16)
            .map_err(|_| Error::new(format!("invalid hex integer {value:?}")))?;
        Plist::Integer(n.into())
    } else if let Some(b64) = value.strip_prefix("@base64:") {
        Plist::Data(
            STANDARD
                .decode(b64)
                .map_err(|_| Error::new(format!("invalid base64 payload in {value:?}")))?,
        )
    } else if let Some(hex) = value.strip_prefix("@hex:") {
        Plist::Data(decode_hex(hex)?)
    } else if let Ok(i) = value.parse::<i64>() {
        Plist::Integer(i.into())
    } else if let Ok(f) = value.parse::<f64>() {
        if value.contains('.') || value.contains('e') || value.contains('E') {
            Plist::Real(f)
        } else {
            Plist::String(value.to_string())
        }
    } else {
        Plist::String(value.to_string())
    };
    Ok((key.to_string(), value))
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(Error::new("odd-length @hex: data"));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        out.push(
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| Error::new(format!("invalid hex byte {}", &hex[i..i + 2])))?,
        );
    }
    Ok(out)
}

/// Wrap a list of key/value pairs into a dictionary.
pub fn dict_from_args(args: &[String]) -> Result<Plist> {
    let mut entries = Dictionary::new();
    for arg in args {
        let (k, v) = value_from_arg(arg)?;
        if entries.contains_key(&k) {
            return Err(Error::new(format!("duplicate plist key {k:?}")));
        }
        entries.insert(k, v);
    }
    Ok(Plist::Dictionary(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(entries: &[(&str, Plist)]) -> Plist {
        Plist::Dictionary(
            entries
                .iter()
                .cloned()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    #[test]
    fn auto_detects_binary_and_xml() {
        let value = dict(&[
            ("CFBundleExecutable", Plist::String("Demo".into())),
            ("count", Plist::Integer(42.into())),
            ("high", Plist::Boolean(true)),
            ("pi", Plist::Real(3.5)),
            ("payload", Plist::Data(vec![0, 1, 2])),
        ]);

        let bin = bplist::encode(&value).unwrap();
        assert_eq!(parse_auto(&bin).unwrap(), value);

        let mut xml = Vec::new();
        value.to_writer_xml(&mut xml).unwrap();
        assert!(xml.starts_with(b"<?xml"));
        assert_eq!(parse_auto(&xml).unwrap(), value);
    }

    #[test]
    fn value_arg_coercion() {
        assert_eq!(
            value_from_arg("flag=true").unwrap(),
            ("flag".into(), Plist::Boolean(true))
        );
        assert_eq!(
            value_from_arg("n=-12").unwrap(),
            ("n".into(), Plist::Integer((-12).into()))
        );
        assert_eq!(
            value_from_arg("hex=0xff").unwrap(),
            ("hex".into(), Plist::Integer(255.into()))
        );
        assert_eq!(
            value_from_arg("data=@base64:TQ==").unwrap(),
            ("data".into(), Plist::Data(b"M".to_vec()))
        );
        assert_eq!(
            value_from_arg("bytes=@hex:00ff10").unwrap(),
            ("bytes".into(), Plist::Data(vec![0x00, 0xff, 0x10]))
        );
        assert_eq!(
            value_from_arg("pi=3.5").unwrap(),
            ("pi".into(), Plist::Real(3.5))
        );
        assert_eq!(
            value_from_arg("name=mosbsfol").unwrap(),
            ("name".into(), Plist::String("mosbsfol".into()))
        );
    }
}
