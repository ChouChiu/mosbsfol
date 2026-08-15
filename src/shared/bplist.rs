// SPDX-License-Identifier: Apache-2.0

//! Minimal Core Foundation binary property list (`bplist00`) codec.
//!
//! Apple's binary plist format is the one used inside modern `.DS_Store`
//! records such as `bwsp`, `icvp` and `lsvp`, and as the value of most
//! `com.apple.metadata:*` extended attributes.

use crate::shared::util::{Error, Result};

pub const BPLIST_MAGIC: &[u8; 8] = b"bplist00";

#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Date(f64),
    Data(Vec<u8>),
    String(String),
    Array(Vec<Plist>),
    Dict(Vec<(String, Plist)>),
}

impl Plist {
    pub fn dict() -> Self {
        Plist::Dict(Vec::new())
    }

    pub fn array() -> Self {
        Plist::Array(Vec::new())
    }

    pub fn string(s: impl Into<String>) -> Self {
        Plist::String(s.into())
    }

    pub fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Plist::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Plist::Int(v) => Some(*v),
            Plist::Real(v) if v.fract() == 0.0 => Some(*v as i64),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
enum Raw {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Date(f64),
    Data(Vec<u8>),
    Str(String),
    Array(Vec<u64>),
    Dict(Vec<(u64, u64)>),
}

/// Number of bytes needed for a signed two's-complement integer.
fn int_bytes(v: i64) -> usize {
    for n in [1usize, 2usize, 4usize, 8] {
        let lo = -(1i128 << (8 * n - 1));
        let hi = (1i128 << (8 * n - 1)) - 1;
        if (v as i128) >= lo && (v as i128) <= hi {
            return n;
        }
    }
    8
}

fn write_uint_be(out: &mut Vec<u8>, v: u64, n: usize) {
    let bytes = v.to_be_bytes();
    out.extend_from_slice(&bytes[8 - n..]);
}

fn write_int(out: &mut Vec<u8>, v: i64) {
    let n = int_bytes(v);
    out.push(0x10 | n.trailing_zeros() as u8);
    let bits = (v as i128) as u128;
    let mask = (1u128 << (8 * n)) - 1;
    write_uint_be(out, (bits & mask) as u64, n);
}

fn write_len_prefix(out: &mut Vec<u8>, kind_nibble: u8, count: u64) {
    if count < 15 {
        out.push((kind_nibble << 4) | count as u8);
    } else {
        out.push((kind_nibble << 4) | 0x0f);
        write_int(out, count as i64);
    }
}

fn encode_raw(obj: &Raw, out: &mut Vec<u8>, ref_size: usize) {
    match obj {
        Raw::Null => out.push(0x00),
        Raw::Bool(v) => out.push(if *v { 0x09 } else { 0x08 }),
        Raw::Int(v) => write_int(out, *v),
        Raw::Real(v) => {
            let f = *v as f32;
            if f as f64 == *v {
                out.push(0x22);
                out.extend_from_slice(&f.to_be_bytes());
            } else {
                out.push(0x23);
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        Raw::Date(v) => {
            out.push(0x33);
            out.extend_from_slice(&v.to_be_bytes());
        }
        Raw::Data(bytes) => {
            write_len_prefix(out, 0x4, bytes.len() as u64);
            out.extend_from_slice(bytes);
        }
        Raw::Str(s) => {
            if s.is_ascii() {
                write_len_prefix(out, 0x5, s.len() as u64);
                out.extend_from_slice(s.as_bytes());
            } else {
                let units: Vec<u16> = s.encode_utf16().collect();
                write_len_prefix(out, 0x6, units.len() as u64);
                for u in units {
                    out.extend_from_slice(&u.to_be_bytes());
                }
            }
        }
        Raw::Array(children) => {
            write_len_prefix(out, 0xa, children.len() as u64);
            for child in children {
                write_uint_be(out, *child, ref_size);
            }
        }
        Raw::Dict(entries) => {
            // Binary plist dictionaries store all key references first,
            // then all value references.
            write_len_prefix(out, 0xd, entries.len() as u64);
            for (key_ref, _) in entries {
                write_uint_be(out, *key_ref, ref_size);
            }
            for (_, value_ref) in entries {
                write_uint_be(out, *value_ref, ref_size);
            }
        }
    }
}

fn collect(value: &Plist, out: &mut Vec<Raw>) -> u64 {
    let raw = match value {
        Plist::Null => Raw::Null,
        Plist::Bool(b) => Raw::Bool(*b),
        Plist::Int(i) => Raw::Int(*i),
        Plist::Real(r) => Raw::Real(*r),
        Plist::Date(d) => Raw::Date(*d),
        Plist::Data(d) => Raw::Data(d.clone()),
        Plist::String(s) => Raw::Str(s.clone()),
        Plist::Array(items) => {
            let mut children = Vec::with_capacity(items.len());
            for item in items {
                children.push(collect(item, out));
            }
            Raw::Array(children)
        }
        Plist::Dict(entries) => {
            // Binary plists conventionally keep dictionary keys sorted.
            let mut sorted = entries.clone();
            sorted.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

            // Collect the key string objects first, then every value.
            let mut key_refs = Vec::with_capacity(sorted.len());
            for (key, _) in &sorted {
                out.push(Raw::Str(key.clone()));
                key_refs.push((out.len() - 1) as u64);
            }
            let mut pairs = Vec::with_capacity(sorted.len());
            for ((_, value), key_ref) in sorted.iter().zip(key_refs) {
                let value_ref = collect(value, out);
                pairs.push((key_ref, value_ref));
            }
            Raw::Dict(pairs)
        }
    };
    out.push(raw);
    (out.len() - 1) as u64
}

/// Encode a plist as a `bplist00` byte string.
pub fn encode(root: &Plist) -> Result<Vec<u8>> {
    let mut raw_objects: Vec<Raw> = Vec::new();
    let top = collect(root, &mut raw_objects);

    let object_count = raw_objects.len();
    let ref_size = if object_count <= 0x100 {
        1
    } else if object_count <= 0x10000 {
        2
    } else {
        4
    };

    let mut object_bytes = Vec::new();
    let mut offsets: Vec<u64> = Vec::with_capacity(object_count);
    for raw in &raw_objects {
        offsets.push(8 + object_bytes.len() as u64);
        encode_raw(raw, &mut object_bytes, ref_size);
    }

    // Chose the narrowest offset-table entry that can address the trailer.
    let mut offset_size = 1usize;
    for k in [1usize, 2, 4, 8] {
        let total = 8 + object_bytes.len() + object_count * k + 32;
        if total <= (1u128 << (8 * k)) as usize {
            offset_size = k;
            break;
        }
    }

    let offset_table_offset = 8 + object_bytes.len();
    let mut out = Vec::with_capacity(8 + object_bytes.len() + object_count * offset_size + 32);
    out.extend_from_slice(BPLIST_MAGIC);
    out.extend_from_slice(&object_bytes);
    for off in &offsets {
        write_uint_be(&mut out, *off, offset_size);
    }
    let mut trailer = Vec::with_capacity(32);
    trailer.extend_from_slice(&[0u8; 6]);
    trailer.push(offset_size as u8);
    trailer.push(ref_size as u8);
    trailer.extend_from_slice(&(object_count as u64).to_be_bytes());
    trailer.extend_from_slice(&top.to_be_bytes());
    trailer.extend_from_slice(&(offset_table_offset as u64).to_be_bytes());
    out.extend_from_slice(&trailer);
    Ok(out)
}

/// Decode a binary plist.
pub fn decode(data: &[u8]) -> Result<Plist> {
    if data.len() < 40 || &data[..8] != BPLIST_MAGIC {
        return Err(Error::new("not a binary property list (bplist00)"));
    }
    let trailer = &data[data.len() - 32..];
    let offset_size = trailer[6] as usize;
    let ref_size = trailer[7] as usize;
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) {
        return Err(Error::new("corrupt bplist trailer sizes"));
    }
    let num_objects = u64::from_be_bytes(trailer[8..16].try_into().unwrap()) as usize;
    let top_object = u64::from_be_bytes(trailer[16..24].try_into().unwrap()) as usize;
    let off_table = u64::from_be_bytes(trailer[24..32].try_into().unwrap()) as usize;
    if top_object >= num_objects {
        return Err(Error::new("bplist top object index out of range"));
    }
    let table_bytes = num_objects
        .checked_mul(offset_size)
        .ok_or_else(|| Error::new("bplist offset table size overflow"))?;
    if off_table
        .checked_add(table_bytes)
        .is_none_or(|end| end > data.len() - 32)
    {
        return Err(Error::new("bplist offset table out of range"));
    }

    let mut starts = Vec::with_capacity(num_objects);
    for i in 0..num_objects {
        let at = off_table + i * offset_size;
        let raw = &data[at..at + offset_size];
        let mut v = 0u64;
        for b in raw {
            v = (v << 8) | *b as u64;
        }
        starts.push(v as usize);
    }

    fn object_end(data: &[u8], starts: &[usize], off_table: usize, idx: usize) -> usize {
        let _ = data;
        let start = starts[idx];
        let mut end = off_table;
        for &s in starts {
            if s > start && s < end {
                end = s;
            }
        }
        end
    }

    fn parse_one(data: &[u8], starts: &[usize], off_table: usize, idx: usize) -> Result<Plist> {
        let start = starts[idx];
        let end = object_end(data, starts, off_table, idx);
        let bytes = data
            .get(start..end)
            .ok_or_else(|| Error::new("bplist object range out of bounds"))?;
        parse_bytes(data, starts, off_table, bytes, 0)
    }

    fn read_length(bytes: &[u8], pos: &mut usize, info: u8) -> Result<u64> {
        if info != 0x0f {
            return Ok(info as u64);
        }
        let marker = *bytes
            .get(*pos)
            .ok_or_else(|| Error::new("truncated bplist length"))?;
        *pos += 1;
        if marker >> 4 != 1 {
            return Err(Error::new("invalid bplist length marker"));
        }
        let n = 1usize << (marker & 0x0f);
        if bytes.len() < *pos + n {
            return Err(Error::new("truncated bplist length body"));
        }
        let mut v = 0u64;
        for b in &bytes[*pos..*pos + n] {
            v = (v << 8) | *b as u64;
        }
        *pos += n;
        Ok(v)
    }

    fn read_ref(trailer_ref_size: usize, bytes: &[u8], pos: &mut usize) -> Result<usize> {
        if bytes.len() < *pos + trailer_ref_size {
            return Err(Error::new("truncated bplist object reference"));
        }
        let mut v = 0usize;
        for b in &bytes[*pos..*pos + trailer_ref_size] {
            v = (v << 8) | *b as usize;
        }
        *pos += trailer_ref_size;
        Ok(v)
    }

    fn parse_bytes(
        data: &[u8],
        starts: &[usize],
        off_table: usize,
        bytes: &[u8],
        depth: usize,
    ) -> Result<Plist> {
        if depth > 64 {
            return Err(Error::new("bplist nesting too deep"));
        }
        let marker = *bytes
            .first()
            .ok_or_else(|| Error::new("empty bplist object"))?;
        let kind = marker >> 4;
        let info = marker & 0x0f;
        let mut pos = 1usize;
        let ref_size = data[data.len() - 32 + 7] as usize;
        match kind {
            0x0 => match marker {
                0x00 => Ok(Plist::Null),
                0x08 => Ok(Plist::Bool(false)),
                0x09 => Ok(Plist::Bool(true)),
                _ => Err(Error::new(format!("unknown bplist marker 0x{marker:02x}"))),
            },
            0x1 => {
                let n = 1usize << info;
                if bytes.len() < pos + n {
                    return Err(Error::new("truncated bplist integer"));
                }
                let mut u = 0u64;
                for b in &bytes[pos..pos + n] {
                    u = (u << 8) | *b as u64;
                }
                let v = if n == 8 {
                    u as i64
                } else {
                    let sign = 1u64 << (8 * n - 1);
                    if u & sign != 0 {
                        u as i64 | !((1i128 << (8 * n)) - 1) as i64
                    } else {
                        u as i64
                    }
                };
                Ok(Plist::Int(v))
            }
            0x2 => {
                let n = 1usize << info;
                if bytes.len() < pos + n {
                    return Err(Error::new("truncated bplist real"));
                }
                let v = if n == 4 {
                    let b: [u8; 4] = bytes[pos..pos + 4].try_into().unwrap();
                    f32::from_be_bytes(b) as f64
                } else if n == 8 {
                    let b: [u8; 8] = bytes[pos..pos + 8].try_into().unwrap();
                    f64::from_be_bytes(b)
                } else {
                    return Err(Error::new(format!(
                        "unsupported bplist real width ({n} bytes)"
                    )));
                };
                Ok(Plist::Real(v))
            }
            0x3 => {
                if info != 0x3 || bytes.len() < 9 {
                    return Err(Error::new("unsupported bplist date encoding"));
                }
                let b: [u8; 8] = bytes[1..9].try_into().unwrap();
                Ok(Plist::Date(f64::from_be_bytes(b)))
            }
            0x4 => {
                let len = read_length(bytes, &mut pos, info)? as usize;
                if bytes.len() < pos + len {
                    return Err(Error::new("truncated bplist data"));
                }
                Ok(Plist::Data(bytes[pos..pos + len].to_vec()))
            }
            0x5 => {
                let len = read_length(bytes, &mut pos, info)? as usize;
                if bytes.len() < pos + len {
                    return Err(Error::new("truncated bplist ascii string"));
                }
                Ok(Plist::String(
                    String::from_utf8_lossy(&bytes[pos..pos + len]).into_owned(),
                ))
            }
            0x6 => {
                let chars = read_length(bytes, &mut pos, info)? as usize;
                if bytes.len() < pos + chars * 2 {
                    return Err(Error::new("truncated bplist utf16 string"));
                }
                let mut units = Vec::with_capacity(chars);
                for w in bytes[pos..pos + chars * 2].chunks_exact(2) {
                    units.push(u16::from_be_bytes([w[0], w[1]]));
                }
                Ok(Plist::String(String::from_utf16_lossy(&units)))
            }
            0x8 => {
                let n = 1 + info as usize;
                if bytes.len() < pos + n {
                    return Err(Error::new("truncated bplist uid"));
                }
                let mut u = 0u64;
                for b in &bytes[pos..pos + n] {
                    u = (u << 8) | *b as u64;
                }
                Ok(Plist::Int(u as i64))
            }
            0xa | 0xc => {
                let count = read_length(bytes, &mut pos, info)? as usize;
                if count > (bytes.len().saturating_sub(pos)) / ref_size.max(1) {
                    return Err(Error::new("bplist array reference count out of range"));
                }
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    let r = read_ref(ref_size, bytes, &mut pos)?;
                    items.push(parse_one(data, starts, off_table, r)?);
                }
                Ok(Plist::Array(items))
            }
            0xd => {
                let count = read_length(bytes, &mut pos, info)? as usize;
                if count > (bytes.len().saturating_sub(pos)) / (2 * ref_size.max(1)) {
                    return Err(Error::new("bplist dictionary reference count out of range"));
                }
                let mut key_refs = Vec::with_capacity(count);
                let mut value_refs = Vec::with_capacity(count);
                for _ in 0..count {
                    key_refs.push(read_ref(ref_size, bytes, &mut pos)?);
                }
                for _ in 0..count {
                    value_refs.push(read_ref(ref_size, bytes, &mut pos)?);
                }
                let mut entries = Vec::with_capacity(count);
                for (k, v) in key_refs.into_iter().zip(value_refs) {
                    let key = match parse_one(data, starts, off_table, k)? {
                        Plist::String(s) => s,
                        _ => return Err(Error::new("bplist dictionary key is not a string")),
                    };
                    entries.push((key, parse_one(data, starts, off_table, v)?));
                }
                Ok(Plist::Dict(entries))
            }
            _ => Err(Error::new(format!(
                "unsupported bplist object type 0x{kind:x}"
            ))),
        }
    }

    parse_one(data, &starts, off_table, top_object)
}

/// Render a plist as compact JSON.  `<data>` becomes a quoted hex string.
pub fn to_json(value: &Plist) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Plist, out: &mut String) {
    match value {
        Plist::Null => out.push_str("null"),
        Plist::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Plist::Int(i) => out.push_str(&i.to_string()),
        Plist::Real(r) => out.push_str(&r.to_string()),
        Plist::Date(d) => out.push_str(&format!("\"<date {d}>\"")),
        Plist::Data(d) => out.push_str(&format!(
            "\"<{} bytes: {}>\"",
            d.len(),
            crate::shared::util::hex_dump(d, 16)
        )),
        Plist::String(s) => out.push_str(&json_string(s)),
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
        Plist::Dict(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_string(k));
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: Plist) {
        let bytes = encode(&value).unwrap();
        assert_eq!(&bytes[..8], BPLIST_MAGIC);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn simple_values() {
        roundtrip(Plist::Bool(true));
        roundtrip(Plist::Bool(false));
        roundtrip(Plist::Int(0));
        roundtrip(Plist::Int(-1));
        roundtrip(Plist::Int(0x123456789abcdef0));
        roundtrip(Plist::Real(48.0));
        roundtrip(Plist::Real(std::f64::consts::PI));
        roundtrip(Plist::Data(vec![1, 2, 3]));
        roundtrip(Plist::String("hello".into()));
        roundtrip(Plist::String("你好 😀".into()));
    }

    #[test]
    fn containers() {
        let value = Plist::Dict(vec![
            ("z".into(), Plist::Int(1)),
            ("a".into(), Plist::Array(vec![Plist::String("x".into())])),
            (
                "bwsp".into(),
                Plist::Dict(vec![(
                    "WindowBounds".into(),
                    Plist::String("{{1, 2}, {3, 4}}".into()),
                )]),
            ),
        ]);
        let bytes = encode(&value).unwrap();
        let decoded = decode(&bytes).unwrap();
        // Dictionary keys are sorted by the binary-plist encoder.
        assert_eq!(decoded.get("z"), Some(&Plist::Int(1)));
        assert_eq!(
            decoded.get("a"),
            Some(&Plist::Array(vec![Plist::String("x".into())]))
        );
        assert!(decoded.get("bwsp").is_some());
    }

    #[test]
    fn many_objects_force_multibyte_refs() {
        let value = Plist::Array((0..300).map(Plist::Int).collect());
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
