// SPDX-License-Identifier: Apache-2.0

//! AppleDouble `._name` sidecar writer/reader.
//!
//! macOS creates these files on filesystems that cannot store resource
//! forks or Finder metadata (FAT/exFAT USB sticks, SMB shares, zip
//! archives).  The layout follows RFC 1740 appendix B:
//!
//! magic `00 05 16 07`, version `00 02 00 00`, 16 filler bytes,
//! u16 entry count, then (id, offset, length) triples.

#[cfg(all(feature = "appledouble", feature = "dsstore"))]
pub mod cli;

use std::fs;
use std::path::{Path, PathBuf};

use crate::shared::fs::{self as fsx, file_name};
use crate::shared::util::{Error, Result};

pub use crate::shared::mac::make_finder_info;

pub const APPLEDOUBLE_MAGIC: [u8; 4] = [0x00, 0x05, 0x16, 0x07];
pub const APPLEDOUBLE_VERSION: [u8; 4] = [0x00, 0x02, 0x00, 0x00];

pub const ENTRY_RESOURCE_FORK: u32 = 2;
pub const ENTRY_FINDER_INFO: u32 = 9;

const FIXED_HEADER_LEN: usize = 26;
const ENTRY_LEN: usize = 12;
const FINDER_INFO_LEN: usize = 32;

/// Serialise an AppleDouble file containing Finder info and an optional
/// resource fork.
pub fn encode(finder_info: &[u8; 32], resource_fork: &[u8]) -> Result<Vec<u8>> {
    debug_assert_eq!(finder_info.len(), FINDER_INFO_LEN);
    let entry_count = u16::from(!resource_fork.is_empty()) + 1;
    let header_len = FIXED_HEADER_LEN + ENTRY_LEN * entry_count as usize;
    let finder_offset = (header_len + 3) & !3; // 4-byte aligned
    let resource_offset = finder_offset + FINDER_INFO_LEN;
    let resource_len = u32::try_from(resource_fork.len())
        .map_err(|_| Error::new("AppleDouble resource fork larger than 4 GiB"))?;
    let total = resource_offset + resource_fork.len();

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&APPLEDOUBLE_MAGIC);
    out.extend_from_slice(&APPLEDOUBLE_VERSION);
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&entry_count.to_be_bytes());

    out.extend_from_slice(&ENTRY_FINDER_INFO.to_be_bytes());
    out.extend_from_slice(&(finder_offset as u32).to_be_bytes());
    out.extend_from_slice(&(FINDER_INFO_LEN as u32).to_be_bytes());

    if !resource_fork.is_empty() {
        out.extend_from_slice(&ENTRY_RESOURCE_FORK.to_be_bytes());
        out.extend_from_slice(&(resource_offset as u32).to_be_bytes());
        out.extend_from_slice(&resource_len.to_be_bytes());
    }

    out.resize(finder_offset, 0);
    out.extend_from_slice(finder_info);
    out.extend_from_slice(resource_fork);
    Ok(out)
}

#[derive(Debug)]
pub struct ParsedAppleDouble {
    pub entries: Vec<(u32, u32, u32)>,
    pub finder_info: Option<[u8; 32]>,
    pub resource_fork: Option<Vec<u8>>,
}

/// Parse an AppleDouble header and payloads.
pub fn parse(data: &[u8]) -> Result<ParsedAppleDouble> {
    if data.len() < FIXED_HEADER_LEN || data[0..4] != APPLEDOUBLE_MAGIC {
        return Err(Error::new("not an AppleDouble file (bad magic)"));
    }
    if data[4..8] != APPLEDOUBLE_VERSION {
        return Err(Error::new(format!(
            "unsupported AppleDouble version {:02x?}",
            &data[4..8]
        )));
    }

    let count = u16::from_be_bytes(data[24..26].try_into().unwrap()) as usize;
    let table_end = FIXED_HEADER_LEN
        .checked_add(
            count
                .checked_mul(ENTRY_LEN)
                .ok_or_else(|| Error::new("AppleDouble entry table length overflow"))?,
        )
        .ok_or_else(|| Error::new("AppleDouble entry table length overflow"))?;
    if data.len() < table_end {
        return Err(Error::new("truncated AppleDouble entry table"));
    }

    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let pos = FIXED_HEADER_LEN + index * ENTRY_LEN;
        let id = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
        let offset = u32::from_be_bytes(data[pos + 4..pos + 8].try_into().unwrap());
        let len = u32::from_be_bytes(data[pos + 8..pos + 12].try_into().unwrap());
        entries.push((id, offset, len));
    }

    let mut finder_info = None;
    let mut resource_fork = None;
    for (id, offset, len) in &entries {
        let start = *offset as usize;
        let end = start
            .checked_add(*len as usize)
            .ok_or_else(|| Error::new("AppleDouble entry range overflow"))?;
        let slice = data
            .get(start..end)
            .ok_or_else(|| Error::new("AppleDouble entry out of file range"))?;
        match *id {
            ENTRY_FINDER_INFO if slice.len() == FINDER_INFO_LEN => {
                finder_info = Some(slice.try_into().unwrap());
            }
            ENTRY_RESOURCE_FORK => resource_fork = Some(slice.to_vec()),
            _ => {}
        }
    }
    Ok(ParsedAppleDouble {
        entries,
        finder_info,
        resource_fork,
    })
}

/// One `._file` sidecar path for `target`.
pub fn sidecar_path(target: &Path) -> PathBuf {
    let name = file_name(target);
    target.with_file_name(format!("{}{}", "._", name))
}

/// Raw Resource Fork data attached to `target`.
///
/// With the `xattr` feature this reads `com.apple.ResourceFork`; filesystems
/// without xattr support yield an empty fork (which is exactly why Apple
/// invented AppleDouble sidecars).  Without the feature the fork is always
/// empty.
pub fn resource_fork_for(target: &Path) -> Result<Vec<u8>> {
    #[cfg(feature = "xattr")]
    {
        crate::features::xattr::resource_fork_or_empty(target)
    }
    #[cfg(not(feature = "xattr"))]
    {
        let _ = target;
        Ok(Vec::new())
    }
}

/// Build the AppleDouble sidecar bytes for one file or directory.
pub fn sidecar_bytes_for(
    target: &Path,
    type_code: &[u8; 4],
    creator_code: &[u8; 4],
) -> Result<Vec<u8>> {
    encode(
        &make_finder_info(type_code, creator_code),
        &resource_fork_for(target)?,
    )
}

/// Create a `._` sidecar for one file or directory.
pub fn create_sidecar(
    target: &Path,
    type_code: &[u8; 4],
    creator_code: &[u8; 4],
) -> Result<PathBuf> {
    let sidecar = sidecar_path(target);
    fs::write(
        &sidecar,
        sidecar_bytes_for(target, type_code, creator_code)?,
    )?;
    Ok(sidecar)
}

/// Create AppleDouble droppings for every file in `root` (and recursively,
/// if requested), like copying a tree onto a FAT USB stick from macOS.
pub fn poop_tree(
    root: &Path,
    recursive: bool,
    include_dirs: bool,
    use_type_codes: bool,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    fsx::require_dir(root)?;
    let mut out = Vec::new();
    poop_tree_inner(
        root,
        recursive,
        include_dirs,
        use_type_codes,
        dry_run,
        &mut out,
    )?;
    Ok(out)
}

fn poop_tree_inner(
    dir: &Path,
    recursive: bool,
    include_dirs: bool,
    use_type_codes: bool,
    dry_run: bool,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    for child in fsx::sorted_dir_entries(dir)? {
        let name = file_name(&child);
        if name.starts_with("._") || name == ".DS_Store" {
            continue;
        }
        let meta = fsx::symlink_metadata(&child)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if recursive {
                poop_tree_inner(
                    &child,
                    recursive,
                    include_dirs,
                    use_type_codes,
                    dry_run,
                    out,
                )?;
            }
            if include_dirs {
                if dry_run {
                    out.push(sidecar_path(&child));
                } else {
                    out.push(create_sidecar(
                        &child,
                        if use_type_codes { b"fold" } else { b"????" },
                        b"MACS",
                    )?);
                }
            }
        } else if meta.is_file() {
            if dry_run {
                out.push(sidecar_path(&child));
            } else {
                let type_code = if use_type_codes {
                    crate::shared::mac::mac_type_for_name(&name)
                } else {
                    *b"????"
                };
                out.push(create_sidecar(&child, &type_code, b"MACS")?);
            }
        }
    }
    Ok(())
}

/// Delete `._*` sidecars recursively.  Returns removed paths.
pub fn clean_tree(root: &Path, recursive: bool, dry_run: bool) -> Result<Vec<PathBuf>> {
    fsx::require_dir(root)?;
    let mut removed = Vec::new();
    clean_tree_inner(root, recursive, dry_run, &mut removed)?;
    Ok(removed)
}

fn clean_tree_inner(
    dir: &Path,
    recursive: bool,
    dry_run: bool,
    removed: &mut Vec<PathBuf>,
) -> Result<()> {
    for path in fsx::sorted_dir_entries(dir)? {
        let name = file_name(&path);
        if name.starts_with("._") && fsx::exists_no_follow(&path)? {
            if dry_run {
                removed.push(path);
            } else {
                fs::remove_file(&path)?;
                removed.push(path);
            }
        } else if recursive && !fsx::is_symlink(&path)? && path.is_dir() {
            clean_tree_inner(&path, recursive, dry_run, removed)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_roundtrip() {
        let info = make_finder_info(b"TEXT", b"ttxt");
        let data = encode(&info, &[0, 1, 2]).unwrap();
        assert_eq!(data[0..4], APPLEDOUBLE_MAGIC);
        assert_eq!(data[4..8], APPLEDOUBLE_VERSION);
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.finder_info.unwrap(), info);
        assert_eq!(parsed.resource_fork.unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn empty_resource_uses_single_entry() {
        let info = [0u8; 32];
        let data = encode(&info, &[]).unwrap();
        assert_eq!(u16::from_be_bytes(data[24..26].try_into().unwrap()), 1);
        assert_eq!(data.len(), 72); // 26 + 12 header, padded to 40, + 32 info
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.finder_info, Some(info));
        assert!(parsed.resource_fork.is_none());
    }

    #[test]
    fn parser_rejects_truncated_ranges() {
        let mut data = encode(&[0u8; 32], b"x").unwrap();
        // Corrupt the resource-fork length to point past EOF.
        let resource_entry_offset = 26 + 12;
        data[resource_entry_offset + 8..resource_entry_offset + 12]
            .copy_from_slice(&0xffff_ff00u32.to_be_bytes());
        assert!(parse(&data).is_err());
    }
}
