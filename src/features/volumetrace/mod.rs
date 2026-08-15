// SPDX-License-Identifier: Apache-2.0

//! Feature `volumetrace`: recreate the "this disk was mounted by a Mac"
//! root-directory droppings.
//!
//! Creates/removes `.Spotlight-V100`, `.fseventsd`, `.Trashes`,
//! `.TemporaryItems`, `.localized`, `.VolumeIcon.icns` and the
//! carriage-return-named `Icon\r` file.

pub mod cli;

use std::fs;
use std::path::{Path, PathBuf};

use crate::shared::util::{unix_now, Error, Result};

fn pseudo_uuid() -> String {
    format!(
        "{:08x}-{:04x}-4{:03x}-9{:03x}-{:012x}",
        unix_now(),
        (unix_now() & 0xffff) as u16,
        std::process::id() & 0xfff,
        (unix_now() & 0xfff) as u16,
        unix_now()
    )
}

/// Header-only placeholder ICNS.  Real custom-icon art is not generated.
pub fn minimal_icns() -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&8u32.to_be_bytes());
    out
}

fn make_dir(path: &Path, dry_run: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    out.push(path.to_path_buf());
    if !dry_run {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn make_file(path: &Path, data: &[u8], dry_run: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    out.push(path.to_path_buf());
    if !dry_run {
        fs::write(path, data)?;
    }
    Ok(())
}

/// Drop macOS volume traces into `root` (normally a mounted volume root).
pub fn poop_volume(root: &Path, dry_run: bool) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Err(Error::new(format!("{} is not a directory", root.display())));
    }
    let mut out = Vec::new();

    let spotlight = root.join(".Spotlight-V100");
    make_dir(&spotlight, dry_run, &mut out)?;
    let store = spotlight.join("Store-V2").join(pseudo_uuid());
    make_dir(&store, dry_run, &mut out)?;

    let fsevents = root.join(".fseventsd");
    make_dir(&fsevents, dry_run, &mut out)?;
    let uuid_file = fsevents.join("fseventsd-uuid");
    let mut data = pseudo_uuid().into_bytes();
    data.push(b'\n');
    make_file(&uuid_file, &data, dry_run, &mut out)?;

    let trashes = root.join(".Trashes");
    make_dir(&trashes, dry_run, &mut out)?;
    let uid = unsafe { libc_getuid() };
    make_dir(&trashes.join(uid.to_string()), dry_run, &mut out)?;

    make_dir(&root.join(".TemporaryItems"), dry_run, &mut out)?;
    make_file(&root.join(".localized"), &[], dry_run, &mut out)?;
    make_file(
        &root.join(".VolumeIcon.icns"),
        &minimal_icns(),
        dry_run,
        &mut out,
    )?;
    // Real Finder folder icon marker: "Icon" followed by carriage return.
    make_file(&root.join("Icon\r"), &minimal_icns(), dry_run, &mut out)?;

    Ok(out)
}

fn remove_path(path: &Path, dry_run: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    out.push(path.to_path_buf());
    if dry_run {
        return Ok(());
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// Remove the markers previously created by [`poop_volume`].
pub fn clean_volume(root: &Path, dry_run: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    // Deepest paths first for tidy dry-run output; remove_dir_all handles
    // non-empty directories regardless of order.
    let nested_spotlight = root.join(".Spotlight-V100/Store-V2").join(pseudo_uuid());
    let _ = nested_spotlight; // UUID changes every run, so clean the parents.

    for dir in [
        root.join(".Spotlight-V100"),
        root.join(".fseventsd"),
        root.join(".Trashes"),
        root.join(".TemporaryItems"),
    ] {
        remove_path(&dir, dry_run, &mut out)?;
    }
    for file in [
        root.join(".localized"),
        root.join(".VolumeIcon.icns"),
        root.join("Icon\r"),
    ] {
        remove_path(&file, dry_run, &mut out)?;
    }
    Ok(out)
}

#[cfg(unix)]
unsafe fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    getuid()
}

#[cfg(not(unix))]
fn libc_getuid() -> u32 {
    501
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icns_has_valid_header() {
        let icns = minimal_icns();
        assert_eq!(&icns[0..4], b"icns");
        assert_eq!(u32::from_be_bytes(icns[4..8].try_into().unwrap()), 8);
    }

    #[test]
    fn volume_traces_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-voltrace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let created = poop_volume(&tmp, false).unwrap();
        assert!(tmp.join(".Spotlight-V100").is_dir());
        assert!(tmp.join(".fseventsd/fseventsd-uuid").is_file());
        assert!(tmp.join(".Trashes").is_dir());
        assert!(tmp.join(".TemporaryItems").is_dir());
        assert!(tmp.join(".localized").is_file());
        assert!(tmp.join(".VolumeIcon.icns").is_file());
        assert!(tmp.join("Icon\r").is_file());
        assert!(!created.is_empty());
        let removed = clean_volume(&tmp, false).unwrap();
        assert!(!removed.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }
}
