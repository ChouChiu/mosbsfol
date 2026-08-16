// SPDX-License-Identifier: Apache-2.0

//! Thin helpers around the battle-tested [`plist`] crate.
//!
//! Apple's binary plist format is the one used inside modern `.DS_Store`
//! records such as `bwsp`, `icvp` and `lsvp`, and as the value of most
//! `com.apple.metadata:*` extended attributes.

use std::fmt::Write as _;
use std::io::Cursor;

use crate::shared::util::{Error, Result};

pub use plist::{Date, Dictionary, Integer, Uid, Value as Plist};

pub const BPLIST_MAGIC: &[u8; 8] = b"bplist00";

fn plist_error(e: plist::Error) -> Error {
    Error::new(e.to_string())
}

/// Encode a plist as a `bplist00` byte string.
pub fn encode(value: &Plist) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    value.to_writer_binary(&mut out).map_err(plist_error)?;
    Ok(out)
}

/// Decode a `bplist00` byte string.
pub fn decode(data: &[u8]) -> Result<Plist> {
    if data.len() < 8 || data[..8] != *BPLIST_MAGIC {
        return Err(Error::new("not a binary property list (bplist00)"));
    }
    Plist::from_reader(Cursor::new(data)).map_err(plist_error)
}

/// Render a plist as compact JSON.  `<data>` becomes a quoted hex string.
pub fn to_json(value: &Plist) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Plist, out: &mut String) {
    match value {
        Plist::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Plist::Dictionary(entries) => {
            out.push('{');
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(key, out);
                out.push(':');
                write_json(item, out);
            }
            out.push('}');
        }
        Plist::Boolean(value) => out.push_str(if *value { "true" } else { "false" }),
        Plist::Data(data) => {
            out.push('"');
            let _ = write!(
                out,
                "<{} bytes: {}>",
                data.len(),
                crate::shared::util::hex_dump(data, 16)
            );
            out.push('"');
        }
        Plist::Date(date) => {
            out.push('"');
            let _ = write!(out, "<date {}>", date.to_xml_format());
            out.push('"');
        }
        Plist::Integer(value) => {
            let _ = write!(out, "{value}");
        }
        Plist::Real(value) if value.is_finite() => {
            let _ = write!(out, "{value}");
        }
        // JSON has no NaN/Infinity literals.
        Plist::Real(_) => out.push_str("null"),
        Plist::String(value) => write_json_string(value, out),
        Plist::Uid(value) => {
            let _ = write!(out, "{}", value.get());
        }
        // `plist::Value` is non-exhaustive; future variants display as null.
        _ => out.push_str("null"),
    }
}

fn write_json_string(value: &str, out: &mut String) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
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

    fn roundtrip(value: Plist) {
        let bytes = encode(&value).unwrap();
        assert_eq!(bytes[..8], *BPLIST_MAGIC);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn simple_values() {
        roundtrip(Plist::Boolean(true));
        roundtrip(Plist::Boolean(false));
        roundtrip(Plist::Integer(0.into()));
        roundtrip(Plist::Integer((-1).into()));
        roundtrip(Plist::Integer(0x1234_5678_9abc_def0u64.into()));
        roundtrip(Plist::Real(48.0));
        roundtrip(Plist::Real(std::f64::consts::PI));
        roundtrip(Plist::Data(vec![1, 2, 3]));
        roundtrip(Plist::String("hello".into()));
        roundtrip(Plist::String("你好 😀".into()));
    }

    #[test]
    fn containers() {
        let value = dict(&[
            ("z", Plist::Integer(1.into())),
            ("a", Plist::Array(vec![Plist::String("x".into())])),
            (
                "bwsp",
                dict(&[("WindowBounds", Plist::String("{{1, 2}, {3, 4}}".into()))]),
            ),
        ]);
        let bytes = encode(&value).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(
            decoded.as_dictionary().and_then(|d| d.get("z")),
            Some(&Plist::Integer(1.into()))
        );
        assert_eq!(
            decoded.as_dictionary().and_then(|d| d.get("a")),
            Some(&Plist::Array(vec![Plist::String("x".into())]))
        );
        assert!(decoded
            .as_dictionary()
            .and_then(|d| d.get("bwsp"))
            .is_some());
    }

    #[test]
    fn json_output_is_valid_json_for_all_values() {
        let value = dict(&[
            ("nan", Plist::Real(f64::NAN)),
            ("inf", Plist::Real(f64::INFINITY)),
            (
                "text",
                Plist::String("quote\" slash\\ newline\n tab\t".into()),
            ),
        ]);
        let json = to_json(&value);
        assert!(json.contains("\"nan\":null"));
        assert!(json.contains("\"inf\":null"));
        assert!(json.contains("\\\""));
        assert!(json.contains("\\n"));
    }

    #[test]
    fn many_objects_force_multibyte_refs() {
        let value = Plist::Array((0..300).map(|i| Plist::Integer(i.into())).collect());
        let bytes = encode(&value).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn reject_garbage() {
        assert!(decode(b"nope").is_err());
        assert!(decode(b"bplist00xxxxxxxx").is_err());
    }

    #[test]
    fn reject_unsupported_real_width_without_panicking() {
        let mut bytes = b"bplist00".to_vec();
        bytes.push(0x21); // real, 2^1 = 2-byte body (invalid)
        bytes.extend_from_slice(&[0x00, 0x00]);
        bytes.push(8); // object 0 starts at offset 8
        bytes.extend_from_slice(&[0u8; 6]);
        bytes.extend_from_slice(&[1, 1]); // offset size, ref size
        bytes.extend_from_slice(&1u64.to_be_bytes()); // one object
        bytes.extend_from_slice(&0u64.to_be_bytes()); // top object
        bytes.extend_from_slice(&11u64.to_be_bytes()); // offset table at 11
        assert!(decode(&bytes).is_err());
    }
}
