// SPDX-License-Identifier: Apache-2.0

//! Small shared helpers.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

/// Library error type.  Dynamic messages are the common case for this
/// small tool; `?` on `std::io::Error` gets the usual transparent display.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Round `n` up to the next power of two (minimum `min`, maximum 2^31).
pub fn align_power_of_two(n: usize, min: usize) -> Result<usize> {
    if n > (1usize << 31) {
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
    let b = name.as_bytes();
    if b.len() != 4 {
        return Err(Error::new(format!(
            "FourCC {name:?} must contain exactly four bytes"
        )));
    }
    let mut out = [0u8; 4];
    for (i, c) in b.iter().enumerate() {
        out[i] = if c.is_ascii() { *c } else { b'?' };
    }
    Ok(u32::from_be_bytes(out))
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
    for w in data.chunks_exact(2) {
        units.push(u16::from_be_bytes([w[0], w[1]]));
    }
    String::from_utf16_lossy(&units)
}

/// Pretty-ish one-line hex dump, used by the DS_Store inspector.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment() {
        assert_eq!(align_power_of_two(0, 32).unwrap(), 32);
        assert_eq!(align_power_of_two(31, 32).unwrap(), 32);
        assert_eq!(align_power_of_two(33, 32).unwrap(), 64);
        assert_eq!(align_power_of_two(4096, 32).unwrap(), 4096);
    }

    #[test]
    fn fourcc_roundtrip() {
        assert_eq!(fourcc_to_u32("blob").unwrap(), 0x626c6f62);
        assert_eq!(fourcc_from_u32(0x626c6f62), "blob");
    }

    #[test]
    fn utf16_roundtrip() {
        let s = "hello-你好-🐔";
        assert_eq!(utf16be_to_string(&utf16be(s)), s);
    }
}
