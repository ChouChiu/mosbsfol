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
use crate::shared::util::{decode_hex, Error, Result};

fn plist_error(e: plist::Error) -> Error {
    Error::new(format!("invalid plist: {e}"))
}

/// Parse data in either binary or XML form.
pub fn parse_auto(data: &[u8]) -> Result<Plist> {
    Plist::from_reader(Cursor::new(data)).map_err(plist_error)
}

/// Read a plist file in either representation.
pub fn read_file(path: &Path) -> Result<Plist> {
    parse_auto(&fs::read(path)?)
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
        let number = u64::from_str_radix(hex, 16)
            .map_err(|_| Error::new(format!("invalid hex integer {value:?}")))?;
        Plist::Integer(number.into())
    } else if let Some(b64) = value.strip_prefix("@base64:") {
        Plist::Data(
            STANDARD
                .decode(b64)
                .map_err(|_| Error::new(format!("invalid base64 payload in {value:?}")))?,
        )
    } else if let Some(hex) = value.strip_prefix("@hex:") {
        Plist::Data(decode_hex(hex, "hex payload")?)
    } else if let Ok(integer) = value.parse::<i64>() {
        Plist::Integer(integer.into())
    } else if let Ok(real) = value.parse::<f64>() {
        if !real.is_finite() {
            return Err(Error::new(format!(
                "plist real value must be finite, got {value:?}"
            )));
        }
        if value.contains('.') || value.contains('e') || value.contains('E') {
            Plist::Real(real)
        } else {
            // Numeric but out of i64 range: keep the user's spelling as a
            // string rather than silently changing its value.
            Plist::String(value.to_string())
        }
    } else {
        Plist::String(value.to_string())
    };
    Ok((key.to_string(), value))
}

/// Wrap a list of key/value pairs into a dictionary.
pub fn dict_from_args(args: &[String]) -> Result<Plist> {
    let mut entries = Dictionary::new();
    for arg in args {
        let (key, value) = value_from_arg(arg)?;
        if entries.contains_key(&key) {
            return Err(Error::new(format!("duplicate plist key {key:?}")));
        }
        entries.insert(key, value);
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
                .map(|(key, value)| (key.to_string(), value))
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
            value_from_arg("big=0xffffffffffffffff").unwrap(),
            ("big".into(), Plist::Integer(u64::MAX.into()))
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

    #[test]
    fn rejects_bad_values() {
        assert!(value_from_arg("x").is_err());
        assert!(value_from_arg("=1").is_err());
        assert!(value_from_arg("x=0xzz").is_err());
        assert!(value_from_arg("x=@hex:0").is_err());
        assert!(value_from_arg("x=nan").is_err());
        assert!(value_from_arg("x=inf").is_err());
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let args = ["a=1".to_string(), "a=2".to_string()];
        assert!(dict_from_args(&args).is_err());
    }
}
