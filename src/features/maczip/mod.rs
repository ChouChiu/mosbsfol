// SPDX-License-Identifier: Apache-2.0

//! Feature `maczip`: reproduce Finder's `__MACOSX/` AppleDouble entries
//! when creating a ZIP archive.

pub mod cli;
pub mod zip;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use self::zip::{write_zip, ZipEntry};

use crate::features::appledouble;
use crate::shared::fs::{self as fsx, file_name};
use crate::shared::mac::{is_macos_volume_marker, mac_type_for_name};
use crate::shared::util::{Error, Result};

/// A fully built `__MACOSX` ZIP: payload entries plus their display names.
#[derive(Debug)]
pub struct MacZipPlan {
    pub entries: Vec<ZipEntry>,
    pub names: Vec<String>,
}

fn relative(root: &Path, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| Error::new(format!("{} is outside {}", path.display(), root.display())))?;
    Ok(rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn sidecar_zip_path(rel: &str) -> String {
    let (parent, name) = match rel.rfind('/') {
        Some(slash) => (&rel[..slash], &rel[slash + 1..]),
        None => ("", rel),
    };
    if parent.is_empty() {
        format!("__MACOSX/._{name}")
    } else {
        format!("__MACOSX/{parent}/._{name}")
    }
}

/// Build a stored ZIP plan: normal data-fork entries plus the Finder
/// `__MACOSX/<dir>/._<file>` AppleDouble entries.
pub fn build_maczip(root: &Path) -> Result<MacZipPlan> {
    fsx::require_dir(root)?;
    let mut plan = MacZipPlan {
        entries: Vec::new(),
        names: Vec::new(),
    };
    collect_dir(root, root, &mut plan)?;
    Ok(plan)
}

fn collect_dir(root: &Path, dir: &Path, plan: &mut MacZipPlan) -> Result<()> {
    for path in fsx::sorted_dir_entries(dir)? {
        let name = file_name(&path);
        if is_macos_volume_marker(&name) {
            continue;
        }
        let meta = fsx::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_dir(root, &path, plan)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }

        let rel = relative(root, &path)?;
        let data = fs::read(&path)?;
        let mode = meta.permissions().mode();
        plan.entries.push(ZipEntry {
            name: rel.clone(),
            data,
            mode,
        });
        plan.names.push(rel.clone());

        let type_code = mac_type_for_name(&name);
        let sidecar = appledouble::sidecar_bytes_for(&path, &type_code, b"MACS")?;
        let sidecar_name = sidecar_zip_path(&rel);
        plan.entries.push(ZipEntry {
            name: sidecar_name.clone(),
            data: sidecar,
            mode: 0o100644,
        });
        plan.names.push(sidecar_name);
    }
    Ok(())
}

/// Serialise a built plan to `output`.
pub fn write_plan(plan: &MacZipPlan, output: &Path) -> Result<()> {
    fs::write(output, write_zip(&plan.entries)?)?;
    Ok(())
}

/// Build and write a `__MACOSX` ZIP in one call.
pub fn write_maczip(root: &Path, output: &Path) -> Result<MacZipPlan> {
    let plan = build_maczip(root)?;
    write_plan(&plan, output)?;
    Ok(plan)
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

        let plan = build_maczip(&tmp).unwrap();
        assert!(plan.names.contains(&"a.txt".to_string()));
        assert!(plan.names.contains(&"sub/b.png".to_string()));
        assert!(plan.names.contains(&"__MACOSX/._a.txt".to_string()));
        assert!(plan.names.contains(&"__MACOSX/sub/._b.png".to_string()));
        assert_eq!(plan.entries.len(), 4);
        assert_eq!(sidecar_zip_path("file"), "__MACOSX/._file");
        assert_eq!(sidecar_zip_path("a/b/file"), "__MACOSX/a/b/._file");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn skips_macos_markers() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-maczip-mark-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join(".DS_Store"), b"x").unwrap();
        fs::write(tmp.join("._a"), b"x").unwrap();
        fs::write(tmp.join("a.txt"), b"a").unwrap();
        let plan = build_maczip(&tmp).unwrap();
        assert_eq!(plan.names.len(), 2); // a.txt + its sidecar
        let _ = fs::remove_dir_all(&tmp);
    }
}
