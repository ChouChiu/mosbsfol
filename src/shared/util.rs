// SPDX-License-Identifier: Apache-2.0

//! Small shared helpers.

use std::fmt;
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

/// Library error type.
///
/// Dynamic messages are the common case for this tool, and `std::io::Error`
/// passes through with its usual transparent display.
#[derive(Debug)]
pub enum Error {
    Message(String),
    Io(std::io::Error),
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(msg) => f.write_str(msg),
            Self::Io(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Message(_) => None,
            Self::Io(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Round `n` up to the next power of two (minimum `min`, maximum 2^31).
pub fn align_power_of_two(n: usize, min: usize) -> Result<usize> {
    const MAX_BLOCK: usize = 1usize << 31;
    if n > MAX_BLOCK {
        return Err(Error::new(format!(
            "block too large for the DS_Store buddy allocator: {n} bytes"
        )));
    }
    let mut size = min.max(1usize);
    while size < n {
        size = size
            .checked_mul(2)
            .ok_or_else(|| Error::new("size overflow"))?;
    }
    Ok(size)
}

/// Unix time as a string of whole seconds (UTC).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Random UUID v4 for quarantine entries, `.fseventsd`, and Spotlight stores.
#[cfg(any(feature = "xattr", feature = "volumetrace"))]
pub fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Convert a 4CC stored in a big-endian u32 to its ASCII name.
pub fn fourcc_from_u32(v: u32) -> String {
    let b = v.to_be_bytes();
    b.iter()
        .map(|c| {
            if (0x20..0x7f).contains(c) {
                *c as char
            } else {
                '.'
            }
        })
        .collect()
}

/// Pack a FourCC string into a big-endian u32.  Non-ASCII bytes become `?`.
pub fn fourcc_to_u32(name: &str) -> Result<u32> {
    Ok(u32::from_be_bytes(fourcc_bytes(name)?))
}

/// Validate a FourCC argument and return its four bytes.
pub fn fourcc_bytes(name: &str) -> Result<[u8; 4]> {
    let b = name.as_bytes();
    if b.len() != 4 {
        return Err(Error::new(format!(
            "FourCC {name:?} must contain exactly four bytes"
        )));
    }
    let mut out = [b'?'; 4];
    for (slot, byte) in out.iter_mut().zip(b) {
        *slot = if byte.is_ascii() { *byte } else { b'?' };
    }
    Ok(out)
}

/// Encode a Rust string as big-endian UTF-16.
pub fn utf16be(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for unit in s.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

/// Decode big-endian UTF-16.  Invalid code units become U+FFFD.
pub fn utf16be_to_string(data: &[u8]) -> String {
    if !data.len().is_multiple_of(2) {
        return String::from_utf16_lossy(&[]);
    }
    let mut units = Vec::with_capacity(data.len() / 2);
    for word in data.chunks_exact(2) {
        units.push(u16::from_be_bytes([word[0], word[1]]));
    }
    String::from_utf16_lossy(&units)
}

/// Decode a hex string into bytes, rejecting odd lengths and bad digits.
pub fn decode_hex(hex: &str, what: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(Error::new(format!("odd-length {what}")));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        out.push(
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| Error::new(format!("invalid {what}")))?,
        );
    }
    Ok(out)
}

/// Pretty-ish one-line hex dump, used by inspectors.
pub fn hex_dump(data: &[u8], max: usize) -> String {
    let mut s = String::new();
    for (i, b) in data.iter().take(max).enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02x}");
    }
    if data.len() > max {
        let _ = write!(s, " ... ({} bytes total)", data.len());
    }
    s
}

/// Parse the boolean spellings accepted by `xattr hide`.
pub fn parse_yes_no(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "on" | "1" => Ok(true),
        "no" | "false" | "off" | "0" => Ok(false),
        other => Err(Error::new(format!("expected yes/no, got {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment() {
        assert_eq!(align_power_of_two(0, 32).unwrap(), 32);
        assert_eq!(align_power_of_two(31, 32).unwrap(), 32);
        assert_eq!(align_power_of_two(33, 32).unwrap(), 64);
        assert_eq!(align_power_of_two(4096, 32).unwrap(), 4096);
        assert!(align_power_of_two((1usize << 31) + 1, 32).is_err());
    }

    #[test]
    fn fourcc_roundtrip() {
        assert_eq!(fourcc_to_u32("blob").unwrap(), 0x626c6f62);
        assert_eq!(fourcc_from_u32(0x626c6f62), "blob");
        assert_eq!(fourcc_bytes("????").unwrap(), *b"????");
        assert_eq!(fourcc_to_u32("aéb").unwrap(), u32::from_be_bytes(*b"a??b"));
    }

    #[test]
    fn utf16_roundtrip() {
        let s = "hello-你好-🐔";
        assert_eq!(utf16be_to_string(&utf16be(s)), s);
    }

    #[test]
    fn hex_decoding() {
        assert_eq!(decode_hex("00ff10", "test").unwrap(), vec![0, 0xff, 0x10]);
        assert!(decode_hex("0", "test").is_err());
        assert!(decode_hex("0g", "test").is_err());
    }

    #[test]
    fn boolean_spellings() {
        for (s, want) in [
            ("yes", true),
            ("ON", true),
            ("1", true),
            ("no", false),
            ("OFF", false),
            ("0", false),
        ] {
            assert_eq!(parse_yes_no(s).unwrap(), want);
        }
        assert!(parse_yes_no("maybe").is_err());
    }
}
