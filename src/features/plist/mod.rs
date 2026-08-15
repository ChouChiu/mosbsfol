// SPDX-License-Identifier: Apache-2.0

//! Property-list file helpers.
//!
//! Reads and writes both the NeXT/Apple XML plist (`plist.5`) and the
//! binary `bplist00` encoding.  The XML parser is intentionally small;
//! it handles the subset emitted by Apple tooling and `.app` bundles.

pub mod cli;

use std::fs;
use std::path::Path;

use crate::shared::bplist::{self, Plist};
use crate::shared::util::{Error, Result};

/// Parse data in either binary or XML form.
pub fn parse_auto(data: &[u8]) -> Result<Plist> {
    if data.starts_with(bplist::BPLIST_MAGIC) {
        bplist::decode(data)
    } else {
        let text = std::str::from_utf8(data)
            .map_err(|_| Error::new("plist is neither bplist00 nor UTF-8 XML"))?;
        xml::parse(text)
    }
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
        xml::serialize(value)?.into_bytes()
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
        Plist::Bool(true)
    } else if value.eq_ignore_ascii_case("false") {
        Plist::Bool(false)
    } else if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let n = i64::from_str_radix(hex, 16)
            .map_err(|_| Error::new(format!("invalid hex integer {value:?}")))?;
        Plist::Int(n)
    } else if let Some(b64) = value.strip_prefix("@base64:") {
        Plist::Data(xml::base64_decode(b64.as_bytes())?)
    } else if let Some(hex) = value.strip_prefix("@hex:") {
        let mut out = Vec::with_capacity(hex.len() / 2);
        if hex.len() % 2 != 0 {
            return Err(Error::new("odd-length @hex: data"));
        }
        for i in (0..hex.len()).step_by(2) {
            out.push(
                u8::from_str_radix(&hex[i..i + 2], 16)
                    .map_err(|_| Error::new(format!("invalid hex byte {}", &hex[i..i + 2])))?,
            );
        }
        Plist::Data(out)
    } else if let Ok(i) = value.parse::<i64>() {
        Plist::Int(i)
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

/// Wrap a list of key/value pairs into a dictionary.
pub fn dict_from_args(args: &[String]) -> Result<Plist> {
    let mut entries = Vec::with_capacity(args.len());
    for arg in args {
        let (k, v) = value_from_arg(arg)?;
        if entries.iter().any(|(ek, _): &(String, Plist)| ek == &k) {
            return Err(Error::new(format!("duplicate plist key {k:?}")));
        }
        entries.push((k, v));
    }
    Ok(Plist::Dict(entries))
}

pub mod xml {
    use super::{Plist, Result};
    use crate::shared::util::Error;

    /// Serialise a plist as Apple/NeXT XML.
    pub fn serialize(value: &Plist) -> Result<String> {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
        out.push_str("<plist version=\"1.0\">\n");
        write_value(value, &mut out, 0);
        out.push_str("</plist>\n");
        Ok(out)
    }

    fn write_value(value: &Plist, out: &mut String, depth: usize) {
        let pad = "\t".repeat(depth);
        let pad1 = "\t".repeat(depth + 1);
        match value {
            Plist::Null => out.push_str(&format!("{pad}<dict/>\n")),
            Plist::Bool(b) => {
                out.push_str(&format!("{pad}<{}/>\n", if *b { "true" } else { "false" }))
            }
            Plist::Int(i) => out.push_str(&format!("{pad}<integer>{i}</integer>\n")),
            Plist::Real(r) => out.push_str(&format!("{pad}<real>{r}</real>\n")),
            Plist::String(s) => out.push_str(&format!("{pad}<string>{}</string>\n", escape(s))),
            Plist::Data(d) => out.push_str(&format!("{pad}<data>{}</data>\n", base64_encode(d))),
            Plist::Date(seconds) => {
                let days = *seconds / 86400.0;
                let whole = days.floor() as i64;
                let time = *seconds - whole as f64 * 86400.0;
                let (h, m, s) = (
                    (time / 3600.0) as u32,
                    ((time % 3600.0) / 60.0) as u32,
                    time % 60.0,
                );
                // 2001-01-01 + `whole` days (approximate; leap days are
                // ignored by this toy serializer).
                out.push_str(&format!(
                    "{pad}<date>2001-01-01T{h:02}:{m:02}:{s:05.2}Z+{whole:05}</date>\n"
                ));
            }
            Plist::Array(items) => {
                out.push_str(&format!("{pad}<array>\n"));
                for item in items {
                    write_value(item, out, depth + 1);
                }
                out.push_str(&format!("{pad}</array>\n"));
            }
            Plist::Dict(entries) => {
                out.push_str(&format!("{pad}<dict>\n"));
                for (k, v) in entries {
                    out.push_str(&format!("{pad1}<key>{}</key>\n", escape(k)));
                    write_value(v, out, depth + 1);
                }
                out.push_str(&format!("{pad}</dict>\n"));
            }
        }
    }

    fn escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(c),
            }
        }
        out
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Token {
        Open(String),
        Close(String),
        Text(String),
        Empty(String),
    }

    fn tokenize(text: &str) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'<' {
                if text[i..].starts_with("<?") || text[i..].starts_with("<!") {
                    let end = text[i..]
                        .find('>')
                        .ok_or_else(|| Error::new("unterminated XML declaration/doctype"))?;
                    i += end + 1;
                    continue;
                }
                let end = text[i..]
                    .find('>')
                    .ok_or_else(|| Error::new("unterminated XML tag"))?;
                let inner = &text[i + 1..i + end];
                if let Some(name) = inner.strip_prefix('/') {
                    tokens.push(Token::Close(name.trim().to_string()));
                } else if let Some(name) = inner.strip_suffix('/') {
                    tokens.push(Token::Empty(name.trim().to_string()));
                } else {
                    let name = inner.split_whitespace().next().unwrap_or("").to_string();
                    if name.is_empty() {
                        return Err(Error::new("empty XML tag"));
                    }
                    tokens.push(Token::Open(name));
                }
                i += end + 1;
            } else {
                let start = i;
                while i < bytes.len() && bytes[i] != b'<' {
                    i += 1;
                }
                let text = decode_entities(&text[start..i]);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    tokens.push(Token::Text(trimmed.to_string()));
                }
            }
        }
        Ok(tokens)
    }

    fn decode_entities(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    }

    struct Cursor {
        tokens: Vec<Token>,
        pos: usize,
    }

    impl Cursor {
        fn peek(&self) -> Result<&Token> {
            self.tokens
                .get(self.pos)
                .ok_or_else(|| Error::new("unexpected end of XML plist"))
        }

        fn next(&mut self) -> Result<Token> {
            let t = self.peek()?.clone();
            self.pos += 1;
            Ok(t)
        }

        fn text(&mut self) -> Result<String> {
            match self.next()? {
                Token::Text(s) => Ok(s),
                other => Err(Error::new(format!("expected XML text, got {other:?}"))),
            }
        }

        fn parse(&mut self) -> Result<Plist> {
            match self.next()? {
                Token::Open(tag) => self.parse_open(tag),
                Token::Empty(tag) => match tag.as_str() {
                    "true" => Ok(Plist::Bool(true)),
                    "false" => Ok(Plist::Bool(false)),
                    other => Err(Error::new(format!("unsupported empty XML tag <{other}/>"))),
                },
                other => Err(Error::new(format!("expected XML element, got {other:?}"))),
            }
        }

        fn parse_open(&mut self, tag: String) -> Result<Plist> {
            let value = match tag.as_str() {
                "plist" => {
                    let v = self.parse()?;
                    expect_close(self, "plist")?;
                    return Ok(v);
                }
                "dict" => {
                    let mut entries = Vec::new();
                    loop {
                        match self.peek()? {
                            Token::Close(name) if name == "dict" => {
                                self.pos += 1;
                                break;
                            }
                            _ => {}
                        }
                        expect_open(self, "key")?;
                        let key = self.text()?;
                        expect_close(self, "key")?;
                        let value = self.parse()?;
                        entries.push((key, value));
                    }
                    Plist::Dict(entries)
                }
                "array" => {
                    let mut items = Vec::new();
                    loop {
                        if let Token::Close(name) = self.peek()? {
                            if name == "array" {
                                self.pos += 1;
                                break;
                            }
                        }
                        items.push(self.parse()?);
                    }
                    Plist::Array(items)
                }
                "string" => {
                    let s = self.text()?;
                    expect_close(self, "string")?;
                    Plist::String(s)
                }
                "integer" => {
                    let t = self.text()?;
                    expect_close(self, "integer")?;
                    Plist::Int(
                        t.parse::<i64>()
                            .map_err(|_| Error::new(format!("invalid XML integer {t:?}")))?,
                    )
                }
                "real" => {
                    let t = self.text()?;
                    expect_close(self, "real")?;
                    Plist::Real(
                        t.parse::<f64>()
                            .map_err(|_| Error::new(format!("invalid XML real {t:?}")))?,
                    )
                }
                "data" => {
                    let t = self.text()?;
                    expect_close(self, "data")?;
                    Plist::Data(base64_decode(t.as_bytes())?)
                }
                "date" => {
                    let t = self.text()?;
                    expect_close(self, "date")?;
                    Plist::String(t)
                }
                other => Err(Error::new(format!("unsupported XML plist tag <{other}>")))?,
            };
            Ok(value)
        }
    }

    fn expect_open(cur: &mut Cursor, name: &str) -> Result<()> {
        match cur.next()? {
            Token::Open(t) if t == name => Ok(()),
            other => Err(Error::new(format!("expected <{name}>, got {other:?}"))),
        }
    }

    fn expect_close(cur: &mut Cursor, name: &str) -> Result<()> {
        match cur.next()? {
            Token::Close(t) if t == name => Ok(()),
            other => Err(Error::new(format!("expected </{name}>, got {other:?}"))),
        }
    }

    /// Parse a NeXT/Apple XML property list.
    pub fn parse(text: &str) -> Result<Plist> {
        let tokens = tokenize(text)?;
        if tokens.is_empty() {
            return Err(Error::new("empty XML plist"));
        }
        let mut cur = Cursor { tokens, pos: 0 };
        expect_open(&mut cur, "plist")?;
        cur.parse()
    }

    pub fn base64_encode(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(n >> 6) as usize & 63] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[n as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    pub fn base64_decode(data: &[u8]) -> Result<Vec<u8>> {
        let clean: Vec<u8> = data
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        if !clean.len().is_multiple_of(4) {
            return Err(Error::new("invalid base64 length"));
        }
        let mut out = Vec::new();
        for chunk in clean.chunks(4) {
            let mut quad = [0u8; 4];
            let mut pads = 0usize;
            for (i, b) in chunk.iter().enumerate() {
                quad[i] = match b {
                    b'A'..=b'Z' => b - b'A',
                    b'a'..=b'z' => b - b'a' + 26,
                    b'0'..=b'9' => b - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    b'=' => {
                        pads += 1;
                        0
                    }
                    _ => return Err(Error::new(format!("invalid base64 byte 0x{b:02x}"))),
                };
            }
            if pads > 2 {
                return Err(Error::new("invalid base64 padding"));
            }
            let n = ((quad[0] as u32) << 18)
                | ((quad[1] as u32) << 12)
                | ((quad[2] as u32) << 6)
                | quad[3] as u32;
            out.push((n >> 16) as u8);
            if pads < 2 {
                out.push((n >> 8) as u8);
            }
            if pads < 1 {
                out.push(n as u8);
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn xml_roundtrip() {
            let value = Plist::Dict(vec![
                (
                    "CFBundleExecutable".to_string(),
                    Plist::String("Demo".to_string()),
                ),
                (
                    "CFBundleVersion".to_string(),
                    Plist::String("1.0".to_string()),
                ),
                (
                    "LSMinimumSystemVersion".to_string(),
                    Plist::String("10.13".to_string()),
                ),
                ("high".to_string(), Plist::Bool(true)),
                ("count".to_string(), Plist::Int(42)),
            ]);
            let text = serialize(&value).unwrap();
            assert!(text.starts_with("<?xml"));
            let back = parse(&text).unwrap();
            assert_eq!(back, value);
        }

        #[test]
        fn base64_vectors() {
            assert_eq!(base64_encode(b"Man"), "TWFu");
            assert_eq!(base64_decode(b"TWFu").unwrap(), b"Man");
            assert_eq!(base64_decode(b"TWE=").unwrap(), b"Ma");
            assert_eq!(base64_decode(b"TQ==").unwrap(), b"M");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_detects_binary() {
        let value = Plist::Dict(vec![("a".to_string(), Plist::Int(1))]);
        let bin = bplist::encode(&value).unwrap();
        assert_eq!(parse_auto(&bin).unwrap(), value);
    }
}
