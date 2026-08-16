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

use crate::shared::fs as fsx;
use crate::shared::util::{uuid_v4, Result};

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
    fsx::require_dir(root)?;
    let mut out = Vec::new();

    let spotlight = root.join(".Spotlight-V100");
    make_dir(&spotlight, dry_run, &mut out)?;
    let store = spotlight.join("Store-V2").join(uuid_v4());
    make_dir(&store, dry_run, &mut out)?;

    let fsevents = root.join(".fseventsd");
    make_dir(&fsevents, dry_run, &mut out)?;
    let uuid_file = fsevents.join("fseventsd-uuid");
    let mut data = uuid_v4().into_bytes();
    data.push(b'\n');
    make_file(&uuid_file, &data, dry_run, &mut out)?;

    let trashes = root.join(".Trashes");
    make_dir(&trashes, dry_run, &mut out)?;
    make_dir(&trashes.join(current_uid().to_string()), dry_run, &mut out)?;

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
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            out.push(path.to_path_buf());
            if dry_run {
                return Ok(());
            }
            if meta.file_type().is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Remove the markers previously created by [`poop_volume`].
pub fn clean_volume(root: &Path, dry_run: bool) -> Result<Vec<PathBuf>> {
    fsx::require_dir(root)?;
    let mut out = Vec::new();

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

/// Current process UID without pulling in `libc` just for `getuid`.
///
/// Linux exposes the effective UID in `/proc/self/status`; fall back to
/// Apple's conventional first-user UID when that interface is unavailable
/// (for example in a minimal container).
fn current_uid() -> u32 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("Uid:")
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|uid| uid.parse::<u32>().ok())
            })
        })
        .unwrap_or(501)
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
        assert_eq!(removed.len(), 7); // parent markers only; nested paths are implied
        assert!(!removed.is_empty());
        assert!(clean_volume(&tmp, false).unwrap().is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dry_run_does_not_report_missing_paths() {
        let tmp =
            std::env::temp_dir().join(format!("mosbsfol-voltrace-dry-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let removed = clean_volume(&tmp, true).unwrap();
        assert!(removed.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }
}
