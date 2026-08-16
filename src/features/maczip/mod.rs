// SPDX-License-Identifier: Apache-2.0

//! Feature `maczip`: reproduce Finder's `__MACOSX/` AppleDouble entries
//! when creating a ZIP archive.

pub mod cli;
pub mod zip;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::features::appledouble;
use crate::shared::mac::{is_macos_volume_marker, mac_type_for_name};
use crate::shared::util::{Error, Result};
use zip::{write_zip, ZipEntry};

fn relative(root: &Path, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| Error::new(format!("{} is outside {}", path.display(), root.display())))?;
    Ok(rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn sidecar_zip_path(rel: &str) -> String {
    let rel_path = Path::new(rel);
    let name = rel_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    match rel_path.parent().and_then(|p| p.to_str()) {
        Some("") | None => format!("__MACOSX/{}{}", "._", name),
        Some(parent) => format!("__MACOSX/{parent}/{}{}", "._", name),
    }
}

/// Build a stored ZIP: normal data-fork entries plus the Finder
/// `__MACOSX/<dir>/._<file>` AppleDouble entries.
pub fn build_maczip(root: &Path) -> Result<(Vec<ZipEntry>, Vec<String>)> {
    if !root.is_dir() {
        return Err(Error::new(format!("{} is not a directory", root.display())));
    }
    let mut entries = Vec::new();
    let mut names = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut children: Vec<PathBuf> = fs::read_dir(&dir)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        children.sort();
        for path in children {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if is_macos_volume_marker(&name) {
                continue;
            }
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = relative(root, &path)?;
            let data = fs::read(&path)?;
            let mode = meta.permissions().mode();
            entries.push(ZipEntry {
                name: rel.clone(),
                data,
                mode,
            });
            names.push(rel.clone());

            let type_code = mac_type_for_name(&name);
            let sidecar = appledouble::sidecar_bytes_for(&path, &type_code, b"MACS")?;
            let sidecar_name = sidecar_zip_path(&rel);
            entries.push(ZipEntry {
                name: sidecar_name.clone(),
                data: sidecar,
                mode: 0o100644,
            });
            names.push(sidecar_name);
        }
    }
    Ok((entries, names))
}

pub fn write_maczip(root: &Path, output: &Path) -> Result<Vec<String>> {
    let (entries, names) = build_maczip(root)?;
    fs::write(output, write_zip(&entries)?)?;
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_macosx_entries() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-maczip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("sub")).unwrap();
        fs::write(tmp.join("a.txt"), b"a").unwrap();
        fs::write(tmp.join("sub/b.png"), b"b").unwrap();

        let (entries, names) = build_maczip(&tmp).unwrap();
        assert!(names.contains(&"a.txt".to_string()));
        assert!(names.contains(&"sub/b.png".to_string()));
        assert!(names.contains(&"__MACOSX/._a.txt".to_string()));
        assert!(names.contains(&"__MACOSX/sub/._b.png".to_string()));
        assert_eq!(entries.len(), 4);
        let _ = fs::remove_dir_all(&tmp);
    }
}
