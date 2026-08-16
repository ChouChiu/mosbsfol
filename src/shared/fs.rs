// SPDX-License-Identifier: Apache-2.0

//! Shared filesystem helpers.
//!
//! The features used to copy the same "read directory, ignore entry
//! errors, sort, beware symlinks" logic around.  This module centralises
//! the small amount of traversal infrastructure that is genuinely shared.
#![allow(dead_code)] // each helper is only referenced by a subset of Cargo features

use std::fs;
use std::path::{Path, PathBuf};

use crate::shared::util::Result;

/// Read a directory into a deterministically sorted path list.
///
/// Unlike the previous ad-hoc loops this propagates every `read_dir`
/// and `DirEntry` error instead of silently dropping broken entries.
pub fn sorted_dir_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    Ok(paths)
}

/// `symlink_metadata` without a million `?` at call sites.
pub fn symlink_metadata(path: &Path) -> Result<fs::Metadata> {
    Ok(fs::symlink_metadata(path)?)
}

/// Whether `path` is a symlink (without following it).
pub fn is_symlink(path: &Path) -> Result<bool> {
    Ok(symlink_metadata(path)?.file_type().is_symlink())
}

/// Whether `path` exists, without following a final symlink.
pub fn exists_no_follow(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

/// File name as lossy UTF-8 (the tool's display convention).
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Verify that `path` exists and is a directory.
pub fn require_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(crate::shared::util::Error::new(format!(
            "{} is not a directory",
            path.display()
        )))
    }
}
