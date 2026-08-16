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

const MAGIC: [u8; 4] = [0, 0, 0, 1];
const BUD1: [u8; 4] = *b"Bud1";
const HEADER_LEN: usize = 36;
const ADDRESS_TABLE_OFFSET: usize = 8;

// Reasonable limits for untrusted input.  Real stores are tiny; these are
// high enough for every generated tree and low enough to make malformed
// files fail before allocating gigabytes.
const MAX_BLOCK_COUNT: usize = 65_536;
const MAX_TOC_ENTRIES: usize = 65_536;
const MAX_TREE_RECORDS: usize = 1_000_000;
const MAX_TREE_DEPTH: usize = 32;
const MAX_FILENAME_CHARS: usize = 1_000_000;

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
        Self::Blob(bytes)
    }

    pub fn ustr(s: &str) -> Self {
        Self::Ustr(s.to_string())
    }

    fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Long(_) => "long",
            Self::Shor(_) => "shor",
            Self::Type(_) => "type",
            Self::Blob(_) => "blob",
            Self::Ustr(_) => "ustr",
            Self::Comp(_) => "comp",
            Self::Dutc(_) => "dutc",
        }
    }

    fn encode_body(&self) -> Result<Vec<u8>> {
        match self {
            Self::Bool(value) => Ok(vec![u8::from(*value)]),
            Self::Long(value) => Ok(value.to_be_bytes().to_vec()),
            Self::Shor(value) => {
                let mut bytes = [0u8; 4];
                bytes[2..].copy_from_slice(&value.to_be_bytes());
                Ok(bytes.to_vec())
            }
            Self::Type(value) => {
                if value.len() != 4 {
                    return Err(Error::new(format!(
                        "DS_Store type payload {value:?} must be a four-byte string"
                    )));
                }
                Ok(value.as_bytes().to_vec())
            }
            Self::Blob(bytes) => {
                let len = u32::try_from(bytes.len())
                    .map_err(|_| Error::new("DS_Store blob larger than 4 GiB"))?;
                let mut out = Vec::with_capacity(4 + bytes.len());
                out.extend_from_slice(&len.to_be_bytes());
                out.extend_from_slice(bytes);
                Ok(out)
            }
            Self::Ustr(value) => {
                let encoded = utf16be(value);
                let chars = u32::try_from(encoded.len() / 2)
                    .map_err(|_| Error::new("DS_Store ustr longer than 4 GiB"))?;
                let mut out = Vec::with_capacity(4 + encoded.len());
                out.extend_from_slice(&chars.to_be_bytes());
                out.extend_from_slice(&encoded);
                Ok(out)
            }
            Self::Comp(value) => Ok(value.to_be_bytes().to_vec()),
            Self::Dutc(value) => Ok(value.to_be_bytes().to_vec()),
        }
    }

    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        out.extend_from_slice(self.type_name().as_bytes());
        out.extend_from_slice(&self.encode_body()?);
        Ok(())
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

    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        let utf16 = utf16be(&self.filename);
        let byte_len = u32::try_from(utf16.len())
            .map_err(|_| Error::new("DS_Store filename longer than 4 GiB"))?;
        out.extend_from_slice(&(byte_len / 2).to_be_bytes());
        out.extend_from_slice(&utf16);
        out.extend_from_slice(self.entry_id.as_bytes());
        self.data.encode(out)
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
        records.sort_by(records_sorted_cmp);
        records
    }

    /// Serialise a Finder-valid `.DS_Store`.
    pub fn write(&self) -> Result<Vec<u8>> {
        let records = self.sorted_records();
        u32::try_from(records.len())
            .map_err(|_| Error::new("too many DS_Store records (maximum 2^32-1)"))?;

        // 1. Leaf B-tree node (P = 0 means "leaf").
        let mut leaf = Vec::new();
        leaf.extend_from_slice(&0u32.to_be_bytes());
        leaf.extend_from_slice(&(records.len() as u32).to_be_bytes());
        for record in &records {
            record.encode(&mut leaf)?;
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

        out[0..4].copy_from_slice(&MAGIC);
        out[4..8].copy_from_slice(&BUD1);
        out[8..12].copy_from_slice(&root_offset.to_be_bytes());
        out[12..16].copy_from_slice(&(root.len() as u32).to_be_bytes());
        out[16..20].copy_from_slice(&root_offset.to_be_bytes());
        // Bytes 20..36 are the unknown/unused trailer of the Bud1 header.

        copy_block(&mut out, &root, root_offset);
        copy_block(&mut out, &dsdb, dsdb_offset);
        copy_block(&mut out, &leaf, leaf_offset);

        Ok(out)
    }

    /// Write `.DS_Store` to a path.
    pub fn write_path(&self, path: &std::path::Path) -> Result<()> {
        std::fs::write(path, self.write()?)?;
        Ok(())
    }

    /// Parse an existing `.DS_Store`.
    pub fn parse(bytes: &[u8]) -> Result<ParsedDsStore> {
        ParsedDsStore::parse(bytes)
    }
}

fn copy_block(out: &mut [u8], block: &[u8], offset: u32) {
    let start = 4 + offset as usize;
    out[start..start + block.len()].copy_from_slice(block);
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
        if bytes.len() < HEADER_LEN || bytes[0..4] != MAGIC || bytes[4..8] != BUD1 {
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
        if allocator_size < 8 {
            return Err(Error::new("corrupt .DS_Store: allocator block too small"));
        }
        if allocator_offset & 0x1f != 0 {
            return Err(Error::new(
                "corrupt .DS_Store: allocator block is not 32-byte aligned",
            ));
        }

        let alloc_start = allocator_offset as usize + 4;
        let alloc_end = alloc_start
            .checked_add(allocator_size as usize)
            .ok_or_else(|| Error::new("allocator block size overflow"))?;
        let alloc = bytes
            .get(alloc_start..alloc_end)
            .ok_or_else(|| Error::new("allocator block out of file range"))?;

        let block_count = be_u32(alloc, 0)? as usize;
        if block_count > MAX_BLOCK_COUNT {
            return Err(Error::new(format!(
                "corrupt .DS_Store: unreasonable block count {block_count}"
            )));
        }
        // +4 unknown bytes at alloc[4..8].
        let address_bytes = block_count
            .checked_mul(4)
            .ok_or_else(|| Error::new("address table size overflow"))?;
        if alloc.len() < ADDRESS_TABLE_OFFSET + address_bytes {
            return Err(Error::new("allocator address table truncated"));
        }
        let mut addresses = Vec::with_capacity(block_count);
        for index in 0..block_count {
            addresses.push(be_u32(alloc, ADDRESS_TABLE_OFFSET + index * 4)?);
        }
        // Address table is padded to a multiple of 256 entries (1024 bytes).
        let padded_entries = block_count.div_ceil(256).max(1) * 256;
        let mut pos = ADDRESS_TABLE_OFFSET
            .checked_add(padded_entries * 4)
            .ok_or_else(|| Error::new("padded address table size overflow"))?;

        let toc_count = be_u32(alloc, pos)? as usize;
        pos += 4;
        if toc_count > MAX_TOC_ENTRIES {
            return Err(Error::new(format!(
                "corrupt .DS_Store: unreasonable TOC entry count {toc_count}"
            )));
        }
        let mut toc = Vec::with_capacity(toc_count);
        for _ in 0..toc_count {
            let len = *alloc
                .get(pos)
                .ok_or_else(|| Error::new("truncated TOC name length"))?
                as usize;
            pos += 1;
            let name_end = pos
                .checked_add(len)
                .ok_or_else(|| Error::new("TOC name length overflow"))?;
            let name_bytes = alloc
                .get(pos..name_end)
                .ok_or_else(|| Error::new("truncated TOC name"))?;
            pos = name_end;
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            let id = be_u32(alloc, pos)?;
            pos += 4;
            toc.push((name, id));
        }

        // 32 freelist buckets; parse and discard them (but only after
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
        let levels = be_u32(dsdb_block, 4)? as usize;
        // Record count at [8], node count at [12], page size at [16].
        if levels > MAX_TREE_DEPTH {
            return Err(Error::new(format!(
                "corrupt .DS_Store: unreasonable B-tree level count {levels}"
            )));
        }
        let mut records = Vec::new();
        let mut visited = vec![false; block_count];
        walk_tree(bytes, &addresses, root_node, &mut records, &mut visited, 0)?;

        Ok(Self {
            records,
            allocator_offset,
            allocator_size,
            block_count: block_count as u32,
            toc,
        })
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

fn walk_tree(
    bytes: &[u8],
    addresses: &[u32],
    node_id: u32,
    out: &mut Vec<DsRecord>,
    visited: &mut [bool],
    depth: usize,
) -> Result<()> {
    if depth > MAX_TREE_DEPTH {
        return Err(Error::new("corrupt .DS_Store: B-tree too deep"));
    }
    let index = node_id as usize;
    if let Some(seen) = visited.get_mut(index) {
        if *seen {
            return Err(Error::new("corrupt .DS_Store: B-tree node cycle"));
        }
        *seen = true;
    }

    let block = read_block(bytes, addresses, node_id)?;
    let p = be_u32(block, 0)?;
    let count = be_u32(block, 4)? as usize;
    if count > MAX_TREE_RECORDS {
        return Err(Error::new(
            "corrupt .DS_Store: unreasonable B-tree record count",
        ));
    }

    if p == 0 {
        // Leaf node.
        let mut pos = 8usize;
        for _ in 0..count {
            let (record, next) = parse_record(block, pos)?;
            push_tree_record(out, record)?;
            pos = next;
        }
        return Ok(());
    }

    // Internal node: the first child pointer precedes the first record,
    // and every record is followed by the pointer to its right subtree.
    // Walk all children in order, not just the final one.
    let mut pos = 8usize;
    let mut child = be_u32(block, pos)?;
    pos += 4;
    walk_tree(bytes, addresses, child, out, visited, depth + 1)?;
    for _ in 0..count {
        let (record, next) = parse_record(block, pos)?;
        pos = next;
        push_tree_record(out, record)?;
        child = be_u32(block, pos)?;
        pos += 4;
        walk_tree(bytes, addresses, child, out, visited, depth + 1)?;
    }
    Ok(())
}

fn push_tree_record(out: &mut Vec<DsRecord>, record: DsRecord) -> Result<()> {
    if out.len() >= MAX_TREE_RECORDS {
        return Err(Error::new("corrupt .DS_Store: too many B-tree records"));
    }
    out.push(record);
    Ok(())
}

fn read_block<'a>(bytes: &'a [u8], addresses: &[u32], block_id: u32) -> Result<&'a [u8]> {
    let packed = *addresses
        .get(block_id as usize)
        .ok_or_else(|| Error::new(format!("block id {block_id} out of address table")))?;
    let offset = addr_offset(packed) as usize;
    let size = 1usize << (packed & 0x1f);
    if size < 32 {
        return Err(Error::new(format!(
            "corrupt .DS_Store: block {block_id} smaller than 32 bytes"
        )));
    }
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
    if name_len > MAX_FILENAME_CHARS {
        return Err(Error::new("unreasonable .DS_Store filename length"));
    }
    let name_end = next
        .checked_add(
            name_len
                .checked_mul(2)
                .ok_or_else(|| Error::new("filename length overflow"))?,
        )
        .ok_or_else(|| Error::new("filename offset overflow"))?;
    let name_bytes = data
        .get(next..name_end)
        .ok_or_else(|| Error::new("truncated .DS_Store filename"))?;
    next = name_end;
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
            let end = next
                .checked_add(len)
                .ok_or_else(|| Error::new("blob length overflow"))?;
            let blob = data
                .get(next..end)
                .ok_or_else(|| Error::new("truncated blob record"))?
                .to_vec();
            next = end;
            DsData::Blob(blob)
        }
        "ustr" => {
            let chars = be_u32(data, next)? as usize;
            next += 4;
            let end = next
                .checked_add(
                    chars
                        .checked_mul(2)
                        .ok_or_else(|| Error::new("ustr length overflow"))?,
                )
                .ok_or_else(|| Error::new("ustr offset overflow"))?;
            let s = data
                .get(next..end)
                .ok_or_else(|| Error::new("truncated ustr record"))?;
            next = end;
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
    if offset & (size - 1) != 0 {
        return Err(Error::new(format!(
            "buddy block offset 0x{offset:x} is not aligned to its size 0x{size:x}"
        )));
    }
    Ok(offset | size.trailing_zeros())
}

fn addr_offset(packed: u32) -> u32 {
    packed & !0x1fu32
}

/// Initial buddy state.  The block for size `2^N` lives at offset `2^N`,
/// which keeps every block naturally aligned and leaves address 0 for the
/// allocator metadata block itself.
fn initial_free_map() -> Vec<FreeBlock> {
    (5..31u32)
        .map(|power| FreeBlock {
            offset: 1 << power,
            size: 1 << power,
        })
        .collect()
}

fn alloc_block(free: &mut Vec<FreeBlock>, size: u32) -> Result<u32> {
    if size < 32 || !size.is_power_of_two() {
        return Err(Error::new(format!(
            "internal error: cannot allocate non-buddy block size {size}"
        )));
    }
    free.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.offset.cmp(&b.offset)));
    let idx = free
        .iter()
        .position(|block| block.size >= size)
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
    debug_assert_eq!(block.size, size);
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
    for power in 0..32u32 {
        let bucket_size = 1u32 << power;
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
    let bytes = data
        .get(pos..pos + 4)
        .ok_or_else(|| Error::new(format!("unexpected EOF at offset {pos}")))?;
    Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
}

fn be_u64(data: &[u8], pos: usize) -> Result<u64> {
    let bytes = data
        .get(pos..pos + 8)
        .ok_or_else(|| Error::new(format!("unexpected EOF at offset {pos}")))?;
    Ok(u64::from_be_bytes(bytes.try_into().unwrap()))
}

/// Human-readable description of a record value.
pub fn display_value(record: &DsRecord) -> String {
    match &record.data {
        DsData::Bool(value) => format!("bool {value}"),
        DsData::Long(value) => format!("long {value}"),
        DsData::Shor(value) => format!("shor {value}"),
        DsData::Type(value) => format!("type {value}"),
        DsData::Comp(value) => format!("comp {value}"),
        DsData::Dutc(value) => {
            // 1/65536-second ticks since the 1904 Mac epoch.
            let secs = *value as f64 / 65536.0;
            format!("dutc {value} ({secs:.3}s since 1904-01-01)")
        }
        DsData::Ustr(value) => format!("ustr {value:?}"),
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

/// Case-insensitive filename, then case-sensitive tie-break, then entry id.
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

    fn encode_leaf(records: &[DsRecord]) -> Vec<u8> {
        let mut leaf = Vec::new();
        leaf.extend_from_slice(&0u32.to_be_bytes());
        leaf.extend_from_slice(&(records.len() as u32).to_be_bytes());
        for record in records {
            record.encode(&mut leaf).unwrap();
        }
        leaf.resize(32, 0);
        leaf
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
        assert_eq!(bytes[0..4], MAGIC);
        assert_eq!(bytes[4..8], BUD1);

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

    #[test]
    fn rejects_misaligned_buddy_block() {
        assert!(pack_address(0x21, 0x20).is_err());
        assert!(pack_address(0x40, 0x18).is_err());
        assert!(pack_address(0x40, 0x40).is_ok());
    }

    #[test]
    fn walks_all_internal_node_children() {
        let left_leaf = rec("a.txt", "Iloc", DsData::Bool(true));
        let separator = rec("m.txt", "Iloc", DsData::Bool(false));
        let right_leaf = rec("z.txt", "Iloc", DsData::Bool(true));
        let left = encode_leaf(std::slice::from_ref(&left_leaf));
        let right = encode_leaf(std::slice::from_ref(&right_leaf));

        // Root block @0x40/0x40, leaf blocks @0x80/0x20 and @0x20/0x20.
        // Address offsets are relative to byte 4 of the file.
        let addresses = [
            0,
            pack_address(0x40, 0x40).unwrap(),
            pack_address(0x80, 0x20).unwrap(),
            pack_address(0x20, 0x20).unwrap(),
        ];
        let mut bytes = vec![0u8; 4 + 0xa0];
        bytes[4 + 0x20..4 + 0x40].copy_from_slice(&right);
        bytes[4 + 0x40..4 + 0x80].copy_from_slice(&{
            let mut root = Vec::new();
            root.extend_from_slice(&1u32.to_be_bytes()); // internal node
            root.extend_from_slice(&1u32.to_be_bytes()); // one separator record
            root.extend_from_slice(&2u32.to_be_bytes()); // left subtree
            separator.encode(&mut root).unwrap();
            root.extend_from_slice(&3u32.to_be_bytes()); // right subtree
            root.resize(0x40, 0);
            root
        });
        bytes[4 + 0x80..4 + 0xa0].copy_from_slice(&left);

        let mut records = Vec::new();
        walk_tree(&bytes, &addresses, 1, &mut records, &mut [false; 4], 0).unwrap();
        let names: Vec<_> = records.iter().map(|r| r.filename.as_str()).collect();
        assert_eq!(names, ["a.txt", "m.txt", "z.txt"]);
    }

    #[test]
    fn parser_rejects_cycles() {
        let addresses = [
            0,
            pack_address(0x20, 0x20).unwrap(),
            pack_address(0x40, 0x20).unwrap(),
        ];
        let mut bytes = vec![0u8; 4 + 0x60];
        // Node 1 points to node 2, which points back to node 1.
        bytes[4 + 0x20..4 + 0x40].copy_from_slice(&{
            let mut node = Vec::new();
            node.extend_from_slice(&1u32.to_be_bytes());
            node.extend_from_slice(&0u32.to_be_bytes());
            node.extend_from_slice(&2u32.to_be_bytes());
            node.resize(0x20, 0);
            node
        });
        bytes[4 + 0x40..4 + 0x60].copy_from_slice(&{
            let mut node = Vec::new();
            node.extend_from_slice(&1u32.to_be_bytes());
            node.extend_from_slice(&0u32.to_be_bytes());
            node.extend_from_slice(&1u32.to_be_bytes());
            node.resize(0x20, 0);
            node
        });
        let mut records = Vec::new();
        assert!(walk_tree(&bytes, &addresses, 1, &mut records, &mut [false; 3], 0).is_err());
    }
}
