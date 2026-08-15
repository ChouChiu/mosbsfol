// SPDX-License-Identifier: Apache-2.0

//! `.DS_Store` reader and writer.
//!
//! Layout implemented here follows the public reverse-engineering notes:
//!   * Mozilla Wiki "DS_Store_File_Format"
//!   * Wim Lewis / Mac-Finder-DSStore "DSStoreFormat.pod"
//!
//! A `.DS_Store` file is:
//!   magic 00 00 00 01
//!   + a 32-byte `Bud1` header pointing at the buddy allocator's metadata
//!     block
//!   + blocks managed by a 2^N buddy allocator
//!   + inside those blocks, an address table, a tiny `DSDB` table of
//!     contents, and a single-node B-tree containing Finder records.

use std::cmp::Ordering;

use crate::shared::bplist::{self, Plist};
use crate::shared::util::{fourcc_from_u32, utf16be, utf16be_to_string, Error, Result};

/// The payload of a single Finder record.
#[derive(Clone, Debug, PartialEq)]
pub enum DsData {
    Bool(bool),
    Long(i32),
    Shor(u16),
    Type(String),
    Blob(Vec<u8>),
    Ustr(String),
    Comp(i64),
    Dutc(u64),
}

impl DsData {
    pub fn blob(bytes: Vec<u8>) -> Self {
        DsData::Blob(bytes)
    }

    pub fn ustr(s: &str) -> Self {
        DsData::Ustr(s.to_string())
    }

    fn type_name(&self) -> &'static str {
        match self {
            DsData::Bool(_) => "bool",
            DsData::Long(_) => "long",
            DsData::Shor(_) => "shor",
            DsData::Type(_) => "type",
            DsData::Blob(_) => "blob",
            DsData::Ustr(_) => "ustr",
            DsData::Comp(_) => "comp",
            DsData::Dutc(_) => "dutc",
        }
    }

    fn encode_body(&self) -> Vec<u8> {
        match self {
            DsData::Bool(v) => vec![*v as u8],
            DsData::Long(v) => v.to_be_bytes().to_vec(),
            DsData::Shor(v) => {
                let mut b = [0u8; 4];
                b[2..].copy_from_slice(&v.to_be_bytes());
                b.to_vec()
            }
            DsData::Type(s) => s.as_bytes().to_vec(),
            DsData::Blob(bytes) => {
                let mut out = Vec::with_capacity(4 + bytes.len());
                out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                out.extend_from_slice(bytes);
                out
            }
            DsData::Ustr(s) => {
                let encoded = utf16be(s);
                let mut out = Vec::with_capacity(4 + encoded.len());
                out.extend_from_slice(&((encoded.len() / 2) as u32).to_be_bytes());
                out.extend_from_slice(&encoded);
                out
            }
            DsData::Comp(v) => v.to_be_bytes().to_vec(),
            DsData::Dutc(v) => v.to_be_bytes().to_vec(),
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.type_name().as_bytes());
        out.extend_from_slice(&self.encode_body());
    }
}

/// A Finder record: one filename + one property.
#[derive(Clone, Debug, PartialEq)]
pub struct DsRecord {
    /// File name in the directory.  The directory itself is `"."`.
    pub filename: String,
    /// FourCC property id, e.g. `Iloc`, `bwsp`, `vstl`.
    pub entry_id: String,
    pub data: DsData,
}

impl DsRecord {
    pub fn new(filename: impl Into<String>, entry_id: &str, data: DsData) -> Result<Self> {
        let entry_id = entry_id.to_string();
        if entry_id.len() != 4 || !entry_id.is_ascii() {
            return Err(Error::new(format!(
                "DS_Store entry id {entry_id:?} must be a four-byte ASCII code"
            )));
        }
        Ok(Self {
            filename: filename.into(),
            entry_id,
            data,
        })
    }

    fn encode(&self, out: &mut Vec<u8>) {
        let utf16 = utf16be(&self.filename);
        out.extend_from_slice(&(utf16.len() as u32 / 2).to_be_bytes());
        out.extend_from_slice(&utf16);
        out.extend_from_slice(self.entry_id.as_bytes());
        self.data.encode(out);
    }
}

#[derive(Clone, Debug)]
pub struct DsStore {
    pub records: Vec<DsRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FreeBlock {
    offset: u32,
    size: u32,
}

impl DsStore {
    pub fn new(records: Vec<DsRecord>) -> Self {
        Self { records }
    }

    fn sorted_records(&self) -> Vec<DsRecord> {
        let mut records = self.records.clone();
        records.sort_by(|a, b| {
            let ak = a.filename.to_lowercase();
            let bk = b.filename.to_lowercase();
            ak.cmp(&bk)
                .then_with(|| a.filename.cmp(&b.filename))
                .then_with(|| a.entry_id.cmp(&b.entry_id))
        });
        records
    }

    /// Serialise a Finder-valid `.DS_Store`.
    pub fn write(&self) -> Result<Vec<u8>> {
        let records = self.sorted_records();

        // 1. Leaf B-tree node (P = 0 means "leaf").
        let mut leaf = Vec::new();
        leaf.extend_from_slice(&0u32.to_be_bytes());
        leaf.extend_from_slice(&(records.len() as u32).to_be_bytes());
        for rec in &records {
            rec.encode(&mut leaf);
        }
        let leaf_size = crate::shared::util::align_power_of_two(leaf.len(), 32)?;
        leaf.resize(leaf_size, 0);

        // 2. DSDB master block: root node, levels, record count, node count,
        //    page size.
        let mut dsdb = Vec::new();
        dsdb.extend_from_slice(&2u32.to_be_bytes()); // leaf block id
        dsdb.extend_from_slice(&0u32.to_be_bytes()); // levels of internal nodes
        dsdb.extend_from_slice(&(records.len() as u32).to_be_bytes());
        dsdb.extend_from_slice(&1u32.to_be_bytes()); // one tree node
        dsdb.extend_from_slice(&0x1000u32.to_be_bytes());
        let dsdb_size = crate::shared::util::align_power_of_two(dsdb.len(), 32)?;
        dsdb.resize(dsdb_size, 0);

        // 3. Buddy allocator free map.  Offsets are relative to byte 4 of the
        //    file (immediately after the 4-byte magic prefix).
        let mut free = initial_free_map();
        let leaf_addr = alloc_block(&mut free, leaf_size as u32)?;
        let dsdb_addr = alloc_block(&mut free, dsdb_size as u32)?;

        // Allocator metadata block references itself as block id 0.
        let mut addresses = vec![0u32, dsdb_addr, leaf_addr];

        // Encode once with a placeholder root address to learn its size.
        let mut root = encode_allocator_block(&addresses, &free);
        let root_size = crate::shared::util::align_power_of_two(root.len(), 32)? as u32;
        root.resize(root_size as usize, 0);

        let root_addr = alloc_block(&mut free, root_size)?;
        addresses[0] = root_addr;
        let mut root = encode_allocator_block(&addresses, &free);
        if root.len() > root_size as usize {
            return Err(Error::new(
                "internal error: allocator metadata grew after allocation",
            ));
        }
        root.resize(root_size as usize, 0);

        let root_offset = addr_offset(root_addr);
        let dsdb_offset = addr_offset(dsdb_addr);
        let leaf_offset = addr_offset(leaf_addr);

        // 4. Assemble the file.
        let file_size = 4usize
            + (root_offset as usize + root.len())
                .max(dsdb_offset as usize + dsdb.len())
                .max(leaf_offset as usize + leaf.len());
        let mut out = vec![0u8; file_size];

        // 4-byte prefix.
        out[0..4].copy_from_slice(&[0, 0, 0, 1]);
        // 32-byte Bud1 header.
        out[4..8].copy_from_slice(b"Bud1");
        out[8..12].copy_from_slice(&root_offset.to_be_bytes());
        out[12..16].copy_from_slice(&(root.len() as u32).to_be_bytes());
        out[16..20].copy_from_slice(&root_offset.to_be_bytes());
        // bytes 20..36 are the 16-byte unknown/unused trailer of the block.
        for b in &mut out[20..36] {
            *b = 0;
        }

        let copy = |dst: &mut [u8], src: &[u8], off: u32| {
            let start = 4 + off as usize;
            dst[start..start + src.len()].copy_from_slice(src);
        };
        copy(&mut out, &root, root_offset);
        copy(&mut out, &dsdb, dsdb_offset);
        copy(&mut out, &leaf, leaf_offset);
        Ok(out)
    }

    /// Write `.DS_Store` to a path.
    pub fn write_path(&self, path: &std::path::Path) -> Result<()> {
        let bytes = self.write()?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Parse an existing `.DS_Store`.
    pub fn parse(bytes: &[u8]) -> Result<ParsedDsStore> {
        ParsedDsStore::parse(bytes)
    }
}

/// A parsed `.DS_Store` plus enough metadata for inspection.
#[derive(Debug)]
pub struct ParsedDsStore {
    pub records: Vec<DsRecord>,
    pub allocator_offset: u32,
    pub allocator_size: u32,
    pub block_count: u32,
    pub toc: Vec<(String, u32)>,
}

impl ParsedDsStore {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 36 || bytes[0..4] != [0, 0, 0, 1] || bytes[4..8] != *b"Bud1" {
            return Err(Error::new("not a .DS_Store file (bad magic)"));
        }
        let allocator_offset = be_u32(bytes, 8)?;
        let allocator_size = be_u32(bytes, 12)?;
        let allocator_offset2 = be_u32(bytes, 16)?;
        if allocator_offset != allocator_offset2 {
            return Err(Error::new(
                "corrupt .DS_Store: duplicate allocator offsets disagree",
            ));
        }
        let alloc_start = allocator_offset as usize + 4;
        let alloc_end = alloc_start
            .checked_add(allocator_size as usize)
            .ok_or_else(|| Error::new("allocator block size overflow"))?;
        let alloc = bytes
            .get(alloc_start..alloc_end)
            .ok_or_else(|| Error::new("allocator block out of file range"))?;

        let block_count = be_u32(alloc, 0)?;
        // +4 unknown bytes at alloc[4..8]
        let mut pos = 8usize;
        if alloc.len() < pos + block_count as usize * 4 {
            return Err(Error::new("allocator address table truncated"));
        }
        let mut addresses = Vec::with_capacity(block_count as usize);
        for i in 0..block_count as usize {
            addresses.push(be_u32(alloc, pos + i * 4)?);
        }
        // Address table is padded to a multiple of 256 entries (1024 bytes).
        let padded_entries = block_count.div_ceil(256).max(1) * 256;
        let padded_end = 8 + padded_entries as usize * 4;
        pos = padded_end;

        let toc_count = be_u32(alloc, pos)? as usize;
        pos += 4;
        let mut toc = Vec::with_capacity(toc_count);
        for _ in 0..toc_count {
            let len = *alloc
                .get(pos)
                .ok_or_else(|| Error::new("truncated TOC name length"))?
                as usize;
            pos += 1;
            let name_bytes = alloc
                .get(pos..pos + len)
                .ok_or_else(|| Error::new("truncated TOC name"))?;
            pos += len;
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            let id = be_u32(alloc, pos)?;
            pos += 4;
            toc.push((name, id));
        }

        // 32 freelist buckets; we parse and discard them (but only after
        // locating them correctly).
        for _ in 0..32 {
            let count = be_u32(alloc, pos)? as usize;
            pos += 4;
            pos = pos
                .checked_add(count * 4)
                .ok_or_else(|| Error::new("freelist size overflow"))?;
        }

        let Some((_, dsdb_id)) = toc.iter().find(|(name, _)| name == "DSDB") else {
            return Err(Error::new(".DS_Store has no DSDB table-of-contents entry"));
        };
        let dsdb_block = read_block(bytes, &addresses, *dsdb_id)?;
        let root_node = be_u32(dsdb_block, 0)?;
        // levels at [4], record count at [8], node count at [12], page size at [16]
        let mut records = Vec::new();
        walk_tree(bytes, &addresses, root_node, &mut records)?;

        Ok(ParsedDsStore {
            records,
            allocator_offset,
            allocator_size,
            block_count,
            toc,
        })
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

fn walk_tree(bytes: &[u8], addresses: &[u32], node_id: u32, out: &mut Vec<DsRecord>) -> Result<()> {
    let block = read_block(bytes, addresses, node_id)?;
    let p = be_u32(block, 0)?;
    let count = be_u32(block, 4)? as usize;
    if p == 0 {
        // Leaf node.
        let mut pos = 8usize;
        for _ in 0..count {
            let (record, next) = parse_record(block, pos)?;
            out.push(record);
            pos = next;
        }
        Ok(())
    } else {
        // Internal node: P is the rightmost child, and `count` records are
        // interleaved with `count` left-side child pointers.
        let mut pos = 8usize;
        let mut child = be_u32(block, pos)?;
        pos += 4;
        for _ in 0..count {
            let (record, next) = parse_record(block, pos)?;
            pos = next;
            out.push(record);
            child = be_u32(block, pos)?;
            pos += 4;
        }
        walk_tree(bytes, addresses, child, out)
    }
}

fn read_block<'a>(bytes: &'a [u8], addresses: &[u32], block_id: u32) -> Result<&'a [u8]> {
    let packed = *addresses
        .get(block_id as usize)
        .ok_or_else(|| Error::new(format!("block id {block_id} out of address table")))?;
    let offset = addr_offset(packed) as usize;
    let size = 1usize << (packed & 0x1f);
    let start = offset
        .checked_add(4)
        .ok_or_else(|| Error::new("block offset overflow"))?;
    let end = start
        .checked_add(size)
        .ok_or_else(|| Error::new("block size overflow"))?;
    bytes
        .get(start..end)
        .ok_or_else(|| Error::new(format!("block {block_id} out of file range")))
}

fn parse_record(data: &[u8], pos: usize) -> Result<(DsRecord, usize)> {
    let name_len = be_u32(data, pos)? as usize;
    let mut next = pos + 4;
    if name_len > 1_000_000 {
        return Err(Error::new("unreasonable .DS_Store filename length"));
    }
    let name_bytes = data
        .get(next..next + name_len * 2)
        .ok_or_else(|| Error::new("truncated .DS_Store filename"))?;
    next += name_len * 2;
    let filename = utf16be_to_string(name_bytes);

    let entry_id = be_u32(data, next)?;
    next += 4;
    let type_id = be_u32(data, next)?;
    next += 4;
    let data_type = fourcc_from_u32(type_id);

    let value = match data_type.as_str() {
        "bool" => {
            let v = *data
                .get(next)
                .ok_or_else(|| Error::new("truncated bool record"))?;
            next += 1;
            DsData::Bool(v != 0)
        }
        "type" => {
            let v = be_u32(data, next)?;
            next += 4;
            DsData::Type(fourcc_from_u32(v))
        }
        "long" => {
            let v = be_u32(data, next)? as i32;
            next += 4;
            DsData::Long(v)
        }
        "shor" => {
            let v = be_u32(data, next)? as u16;
            next += 4;
            DsData::Shor(v)
        }
        "comp" => {
            let v = be_u64(data, next)? as i64;
            next += 8;
            DsData::Comp(v)
        }
        "dutc" => {
            let v = be_u64(data, next)?;
            next += 8;
            DsData::Dutc(v)
        }
        "blob" => {
            let len = be_u32(data, next)? as usize;
            next += 4;
            let blob = data
                .get(next..next + len)
                .ok_or_else(|| Error::new("truncated blob record"))?
                .to_vec();
            next += len;
            DsData::Blob(blob)
        }
        "ustr" => {
            let chars = be_u32(data, next)? as usize;
            next += 4;
            let s = data
                .get(next..next + chars * 2)
                .ok_or_else(|| Error::new("truncated ustr record"))?;
            next += chars * 2;
            DsData::Ustr(utf16be_to_string(s))
        }
        other => {
            return Err(Error::new(format!(
                "unsupported .DS_Store record payload type {other:?}"
            )));
        }
    };

    Ok((
        DsRecord {
            filename,
            entry_id: fourcc_from_u32(entry_id),
            data: value,
        },
        next,
    ))
}

/// `offset | log2(size)`, where offset is relative to byte 4.
fn pack_address(offset: u32, size: u32) -> Result<u32> {
    if size < 32 || !size.is_power_of_two() {
        return Err(Error::new("buddy block size must be a power of two >= 32"));
    }
    Ok(offset | size.trailing_zeros())
}

fn addr_offset(packed: u32) -> u32 {
    packed & !0x1fu32
}

fn initial_free_map() -> Vec<FreeBlock> {
    let mut blocks = Vec::new();
    for i in 5..31u32 {
        let size = 1u32 << i;
        blocks.push(FreeBlock { offset: size, size });
    }
    blocks
}

fn alloc_block(free: &mut Vec<FreeBlock>, size: u32) -> Result<u32> {
    free.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.offset.cmp(&b.offset)));
    let idx = free
        .iter()
        .position(|b| b.size >= size)
        .ok_or_else(|| Error::new("DS_Store buddy allocator exhausted"))?;
    let mut block = free.remove(idx);

    // Buddy-split until the block is exactly the requested size.
    while block.size > size {
        let half = block.size / 2;
        free.push(FreeBlock {
            offset: block.offset + half,
            size: half,
        });
        block.size = half;
    }
    if block.size != size {
        return Err(Error::new("buddy allocation size mismatch"));
    }
    pack_address(block.offset, block.size)
}

fn encode_allocator_block(addresses: &[u32], free: &[FreeBlock]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(addresses.len() as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // unknown, always zero

    // Address table, padded to 256 entries / 1024 bytes.
    for addr in addresses {
        out.extend_from_slice(&addr.to_be_bytes());
    }
    let padded = addresses.len().div_ceil(256).max(1) * 256;
    out.resize(8 + padded * 4, 0);

    // Table of contents: DSDB -> block id 1 (the B-tree master block).
    out.extend_from_slice(&1u32.to_be_bytes());
    out.push(4);
    out.extend_from_slice(b"DSDB");
    out.extend_from_slice(&1u32.to_be_bytes());

    // Buddy freelists: one count + offset list for each 2^N bucket.
    let mut sorted = free.to_vec();
    sorted.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.offset.cmp(&b.offset)));
    for i in 0..32u32 {
        let bucket_size = 1u32 << i;
        let mut offsets = Vec::new();
        for block in &sorted {
            if block.size == bucket_size {
                offsets.push(block.offset);
            }
        }
        out.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
        for off in offsets {
            out.extend_from_slice(&off.to_be_bytes());
        }
    }
    out
}

fn be_u32(data: &[u8], pos: usize) -> Result<u32> {
    let b = data
        .get(pos..pos + 4)
        .ok_or_else(|| Error::new(format!("unexpected EOF at offset {pos}")))?;
    Ok(u32::from_be_bytes(b.try_into().unwrap()))
}

fn be_u64(data: &[u8], pos: usize) -> Result<u64> {
    let b = data
        .get(pos..pos + 8)
        .ok_or_else(|| Error::new(format!("unexpected EOF at offset {pos}")))?;
    Ok(u64::from_be_bytes(b.try_into().unwrap()))
}

/// Human-readable description of a record value.
pub fn display_value(record: &DsRecord) -> String {
    match &record.data {
        DsData::Bool(v) => format!("bool {v}"),
        DsData::Long(v) => format!("long {v}"),
        DsData::Shor(v) => format!("shor {v}"),
        DsData::Type(v) => format!("type {v}"),
        DsData::Comp(v) => format!("comp {v}"),
        DsData::Dutc(v) => {
            // 1/65536-second ticks since the 1904 Mac epoch.
            let secs = *v as f64 / 65536.0;
            format!("dutc {v} ({secs:.3}s since 1904-01-01)")
        }
        DsData::Ustr(v) => format!("ustr {v:?}"),
        DsData::Blob(bytes) => {
            let prefix = crate::shared::util::hex_dump(bytes, 24);
            if matches!(record.entry_id.as_str(), "bwsp" | "icvp" | "lsvp" | "lsvP") {
                match bplist::decode(bytes) {
                    Ok(plist) => format!("blob bplist {} [{}]", bplist::to_json(&plist), prefix),
                    Err(_) => format!("blob {prefix}"),
                }
            } else {
                format!("blob {prefix}")
            }
        }
    }
}

/// Human-readable one-line record description.
pub fn display_record(record: &DsRecord) -> String {
    format!(
        "name={:?} id={} {}",
        record.filename,
        record.entry_id,
        display_value(record)
    )
}

/// Decode a modern DS_Store plist blob into a `Plist`.
pub fn decode_plist_blob(data: &DsData) -> Option<Plist> {
    match data {
        DsData::Blob(bytes) => bplist::decode(bytes).ok(),
        _ => None,
    }
}

/// Convenience comparison for tests/readers.
pub fn records_sorted_cmp(a: &DsRecord, b: &DsRecord) -> Ordering {
    a.filename
        .to_lowercase()
        .cmp(&b.filename.to_lowercase())
        .then_with(|| a.filename.cmp(&b.filename))
        .then_with(|| a.entry_id.cmp(&b.entry_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, id: &str, data: DsData) -> DsRecord {
        DsRecord::new(name, id, data).unwrap()
    }

    #[test]
    fn record_encoding_roundtrip() {
        let records = vec![
            rec(".", "vstl", DsData::Type("icnv".into())),
            rec(
                "hello.txt",
                "Iloc",
                DsData::Blob(vec![
                    0, 0, 0, 40, 0, 0, 0, 30, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0,
                ]),
            ),
            rec(
                "hello.txt",
                "cmmt",
                DsData::Ustr("来自 Finder 的评论".to_string()),
            ),
            rec(".", "fwsw", DsData::Long(140)),
            rec(".", "fwvh", DsData::Shor(500)),
            rec(".", "logS", DsData::Comp(12345)),
            rec(".", "modD", DsData::Dutc(0x1234_5678_9abc_def0)),
            rec("flag", "ICVO", DsData::Bool(true)),
        ];

        let store = DsStore::new(records.clone());
        let bytes = store.write().unwrap();
        assert_eq!(&bytes[0..4], [0, 0, 0, 1]);
        assert_eq!(&bytes[4..8], b"Bud1");

        let parsed = DsStore::parse(&bytes).unwrap();
        assert_eq!(parsed.record_count(), records.len());
        let mut got = parsed.records.clone();
        got.sort_by(records_sorted_cmp);
        let mut want = records;
        want.sort_by(records_sorted_cmp);
        assert_eq!(got, want);
    }

    #[test]
    fn empty_store_is_valid() {
        let bytes = DsStore::new(vec![]).write().unwrap();
        assert!(bytes.len() >= 36);
        let parsed = DsStore::parse(&bytes).unwrap();
        assert_eq!(parsed.record_count(), 0);
    }

    #[test]
    fn allocator_layout() {
        let mut free = initial_free_map();
        let leaf = alloc_block(&mut free, 0x400).unwrap();
        let dsdb = alloc_block(&mut free, 0x20).unwrap();
        let root = alloc_block(&mut free, 0x800).unwrap();
        assert_eq!(addr_offset(leaf), 0x400);
        assert_eq!(addr_offset(dsdb), 0x20);
        assert_eq!(addr_offset(root), 0x800);
    }
}
