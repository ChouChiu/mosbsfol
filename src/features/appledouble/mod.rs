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

use crate::shared::util::{Error, Result};

pub use crate::shared::mac::make_finder_info;

pub const APPLEDOUBLE_MAGIC: [u8; 4] = [0x00, 0x05, 0x16, 0x07];
pub const APPLEDOUBLE_VERSION: [u8; 4] = [0x00, 0x02, 0x00, 0x00];

pub const ENTRY_RESOURCE_FORK: u32 = 2;
pub const ENTRY_FINDER_INFO: u32 = 9;

/// Serialise an AppleDouble file containing Finder info and an optional
/// resource fork.
pub fn encode(finder_info: &[u8; 32], resource_fork: &[u8]) -> Vec<u8> {
    let entry_count = if resource_fork.is_empty() { 1u16 } else { 2u16 };
    let header_len = 26 + 12 * entry_count as usize;
    let finder_off = (header_len + 3) & !3; // 4-byte aligned
    let resource_off = finder_off + 32;
    let total = resource_off + resource_fork.len();

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&APPLEDOUBLE_MAGIC);
    out.extend_from_slice(&APPLEDOUBLE_VERSION);
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&entry_count.to_be_bytes());

    out.extend_from_slice(&ENTRY_FINDER_INFO.to_be_bytes());
    out.extend_from_slice(&(finder_off as u32).to_be_bytes());
    out.extend_from_slice(&32u32.to_be_bytes());

    if !resource_fork.is_empty() {
        out.extend_from_slice(&ENTRY_RESOURCE_FORK.to_be_bytes());
        out.extend_from_slice(&(resource_off as u32).to_be_bytes());
        out.extend_from_slice(&(resource_fork.len() as u32).to_be_bytes());
    }

    out.resize(finder_off, 0);
    out.extend_from_slice(finder_info);
    out.extend_from_slice(resource_fork);
    out
}

#[derive(Debug)]
pub struct ParsedAppleDouble {
    pub entries: Vec<(u32, u32, u32)>,
    pub finder_info: Option<[u8; 32]>,
    pub resource_fork: Option<Vec<u8>>,
}

/// Parse an AppleDouble header and payloads.
pub fn parse(data: &[u8]) -> Result<ParsedAppleDouble> {
    if data.len() < 26 || data[0..4] != APPLEDOUBLE_MAGIC {
        return Err(Error::new("not an AppleDouble file (bad magic)"));
    }
    if data[4..8] != APPLEDOUBLE_VERSION {
        return Err(Error::new(format!(
            "unsupported AppleDouble version {:02x?}",
            &data[4..8]
        )));
    }
    let count = u16::from_be_bytes(data[24..26].try_into().unwrap()) as usize;
    let mut pos = 26usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if data.len() < pos + 12 {
            return Err(Error::new("truncated AppleDouble entry table"));
        }
        let id = be_u32(data, pos);
        let off = be_u32(data, pos + 4) as usize;
        let len = be_u32(data, pos + 8) as usize;
        entries.push((id, off as u32, len as u32));
        pos += 12;
    }

    let mut finder_info = None;
    let mut resource_fork = None;
    for (id, off, len) in &entries {
        let slice = data
            .get(*off as usize..*off as usize + *len as usize)
            .ok_or_else(|| Error::new("AppleDouble entry out of file range"))?;
        match *id {
            ENTRY_FINDER_INFO if slice.len() == 32 => {
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

fn be_u32(data: &[u8], pos: usize) -> u32 {
    u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap())
}

/// One `._file` sidecar path for `target`.
pub fn sidecar_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!("{}{}", "._", name))
}

/// Raw Resource Fork data attached to `target`, when the `xattr` feature
/// is compiled in and the filesystem supports it.
pub fn resource_fork_for(target: &Path) -> Vec<u8> {
    #[cfg(feature = "xattr")]
    {
        crate::features::xattr::get_resource_fork(target).unwrap_or_default()
    }
    #[cfg(not(feature = "xattr"))]
    {
        let _ = target;
        Vec::new()
    }
}

/// Build the AppleDouble sidecar bytes for one file or directory.
pub fn sidecar_bytes_for(
    target: &Path,
    type_code: &[u8; 4],
    creator_code: &[u8; 4],
) -> Result<Vec<u8>> {
    let info = make_finder_info(type_code, creator_code);
    Ok(encode(&info, &resource_fork_for(target)))
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
    let mut children: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    children.sort();

    for child in children {
        let name = child
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with("._") || name == ".DS_Store" {
            continue;
        }
        let meta = fs::symlink_metadata(&child)?;
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
                let t = if use_type_codes {
                    crate::shared::mac::mac_type_for_name(&name)
                } else {
                    *b"????"
                };
                out.push(create_sidecar(&child, &t, b"MACS")?);
            }
        }
    }
    Ok(())
}

/// Delete `._*` sidecars recursively.  Returns removed paths.
pub fn clean_tree(root: &Path, recursive: bool, dry_run: bool) -> Result<Vec<PathBuf>> {
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
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with("._") && path.is_file() {
            if dry_run {
                removed.push(path);
            } else {
                fs::remove_file(&path)?;
                removed.push(path);
            }
        } else if recursive && path.is_dir() {
            let meta = fs::symlink_metadata(&path)?;
            if !meta.file_type().is_symlink() {
                clean_tree_inner(&path, recursive, dry_run, removed)?;
            }
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
        let data = encode(&info, &[0, 1, 2]);
        assert_eq!(data[0..4], APPLEDOUBLE_MAGIC);
        assert_eq!(data[4..8], APPLEDOUBLE_VERSION);
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.finder_info.unwrap(), info);
        assert_eq!(parsed.resource_fork.unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn empty_resource_uses_single_entry() {
        let data = encode(&[0u8; 32], &[]);
        assert_eq!(u16::from_be_bytes(data[24..26].try_into().unwrap()), 1);
        let parsed = parse(&data).unwrap();
        assert!(parsed.finder_info.is_some());
        assert!(parsed.resource_fork.is_none());
    }
}
