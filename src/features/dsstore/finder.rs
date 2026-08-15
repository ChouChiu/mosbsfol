// SPDX-License-Identifier: Apache-2.0

//! Finder-like record generation: the actual "bull shit feature".
//!
//! `make_records` produces the same kinds of records a modern Finder
//! drops into a directory:
//!   * window state (`fwi0`, `fwsw`, `fwvh`, `vstl`, `bwsp`)
//!   * icon view options (`icvo`, `icvt`, `icvp`)
//!   * directory size cache (`logS`, `phyS`, `modD`)
//!   * one `Iloc` icon location per visible file/subdirectory.

use std::fs;
use std::path::{Path, PathBuf};

use super::format::{DsData, DsRecord, DsStore};
use crate::shared::bplist::Plist;
use crate::shared::util::{Error, Result};

/// Seconds between the 1904 Mac epoch and the 1970 Unix epoch.
pub const MAC_EPOCH_DELTA: i64 = 2_082_844_800;

#[derive(Clone, Debug)]
pub struct FinderOptions {
    pub icon_size: u16,
    pub grid_spacing: u16,
    pub text_size: u16,
    pub window_bounds: String,
    pub view_style: String,
}

impl Default for FinderOptions {
    fn default() -> Self {
        Self {
            icon_size: 48,
            grid_spacing: 54,
            text_size: 12,
            window_bounds: "{{44, 100}, {920, 600}}".to_string(),
            view_style: "icnv".to_string(),
        }
    }
}

/// Unix seconds -> Mac `dutc` ticks (1/65536 s since 1904-01-01).
pub fn unix_to_dutc(unix_seconds: i64) -> u64 {
    ((unix_seconds.saturating_add(MAC_EPOCH_DELTA)) as u64).saturating_mul(65536)
}

fn modified_dutc(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| unix_to_dutc(d.as_secs() as i64))
        .unwrap_or(unix_to_dutc(crate::shared::util::unix_now() as i64))
}

/// One visible filesystem entry, pre-filtered.
#[derive(Clone, Debug)]
pub struct VisibleEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_dutc: u64,
}

/// List entries that Finder would bother to describe in `.DS_Store`.
pub fn visible_entries(path: &Path, skip_hidden: bool) -> Result<Vec<VisibleEntry>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_finder_dropping(&name) || (skip_hidden && name.starts_with('.')) {
            continue;
        }
        let meta = entry.metadata()?;
        let is_dir = meta.is_dir();
        out.push(VisibleEntry {
            name,
            is_dir,
            size: meta.len(),
            modified_dutc: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| unix_to_dutc(d.as_secs() as i64))
                .unwrap_or_else(|| unix_to_dutc(crate::shared::util::unix_now() as i64)),
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
}

/// Finder's own droppings are not normally recorded in its own store.
pub fn is_finder_dropping(name: &str) -> bool {
    name == ".DS_Store" || name == ".localized" || name.starts_with("._")
}

fn icon_location_blob(x: u32, y: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&x.to_be_bytes());
    b.extend_from_slice(&y.to_be_bytes());
    b.extend_from_slice(&[0xff; 6]);
    b.extend_from_slice(&[0, 0]);
    b
}

fn window_plist(opts: &FinderOptions) -> Plist {
    Plist::Dict(vec![
        (
            "WindowBounds".to_string(),
            Plist::String(opts.window_bounds.clone()),
        ),
        ("SidebarWidth".to_string(), Plist::Real(180.0)),
        ("ShowSidebar".to_string(), Plist::Bool(true)),
        ("ShowToolbar".to_string(), Plist::Bool(false)),
        ("ShowStatusBar".to_string(), Plist::Bool(true)),
        ("ShowPathbar".to_string(), Plist::Bool(false)),
        (
            "ViewStyle".to_string(),
            Plist::String(opts.view_style.clone()),
        ),
    ])
}

fn icon_view_plist(opts: &FinderOptions) -> Plist {
    Plist::Dict(vec![
        ("viewOptionsVersion".to_string(), Plist::Int(1)),
        ("iconSize".to_string(), Plist::Int(opts.icon_size as i64)),
        ("textSize".to_string(), Plist::Int(opts.text_size as i64)),
        ("arrangeBy".to_string(), Plist::String("none".to_string())),
        ("showIconPreview".to_string(), Plist::Bool(true)),
        ("showItemInfo".to_string(), Plist::Bool(false)),
        ("labelOnBottom".to_string(), Plist::Bool(true)),
        (
            "gridSpacing".to_string(),
            Plist::Int(opts.grid_spacing as i64),
        ),
        ("scrollPositionX".to_string(), Plist::Int(0)),
        ("scrollPositionY".to_string(), Plist::Int(0)),
        ("gridOffsetX".to_string(), Plist::Int(0)),
        ("gridOffsetY".to_string(), Plist::Int(0)),
        ("backgroundColorRed".to_string(), Plist::Real(1.0)),
        ("backgroundColorGreen".to_string(), Plist::Real(1.0)),
        ("backgroundColorBlue".to_string(), Plist::Real(1.0)),
    ])
}

fn legacy_icvo(opts: &FinderOptions) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"icv4");
    b.extend_from_slice(&opts.icon_size.to_be_bytes());
    b.extend_from_slice(b"none");
    b.extend_from_slice(b"botm");
    b.extend_from_slice(&[0u8; 12]);
    b
}

fn fwi0_blob() -> Vec<u8> {
    let mut b = Vec::new();
    for v in [44u16, 100, 600, 1020] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(b"icnv");
    b.extend_from_slice(&[0u8; 4]);
    b
}

fn default_background_blob() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"DefB");
    b.extend_from_slice(&[0u8; 8]);
    b
}

/// Build Finder records for one directory.
pub fn make_records(path: &Path, opts: &FinderOptions, skip_hidden: bool) -> Result<Vec<DsRecord>> {
    let entries = visible_entries(path, skip_hidden)?;
    let logical_size: u64 = entries.iter().map(|e| e.size).sum();
    let physical_size = logical_size.saturating_add(8191) & !8191u64;
    let now_dutc = modified_dutc(path);

    let mut records: Vec<DsRecord> = Vec::new();

    let dir_records: Vec<DsRecord> = vec![
        DsRecord::new(".", "vstl", DsData::Type(opts.view_style.clone()))?,
        DsRecord::new(".", "icvo", DsData::Blob(legacy_icvo(opts)))?,
        DsRecord::new(".", "icvt", DsData::Shor(opts.text_size))?,
        DsRecord::new(
            ".",
            "icvp",
            DsData::Blob(crate::shared::bplist::encode(&icon_view_plist(opts))?),
        )?,
        DsRecord::new(
            ".",
            "bwsp",
            DsData::Blob(crate::shared::bplist::encode(&window_plist(opts))?),
        )?,
        DsRecord::new(".", "fwi0", DsData::Blob(fwi0_blob()))?,
        DsRecord::new(".", "fwsw", DsData::Long(180))?,
        DsRecord::new(".", "fwvh", DsData::Shor(600))?,
        DsRecord::new(".", "BKGD", DsData::Blob(default_background_blob()))?,
        DsRecord::new(".", "ICVO", DsData::Bool(true))?,
        DsRecord::new(".", "vSrn", DsData::Long(1))?,
        DsRecord::new(".", "logS", DsData::Comp(logical_size as i64))?,
        DsRecord::new(".", "phyS", DsData::Comp(physical_size as i64))?,
        DsRecord::new(".", "modD", DsData::Dutc(now_dutc))?,
    ];
    records.extend(dir_records);

    // Place icons on a Finder-ish grid.
    let columns = 12usize;
    for (index, entry) in entries.iter().enumerate() {
        let col = (index % columns) as u32;
        let row = (index / columns) as u32;
        let x = 40 + col * opts.grid_spacing as u32 + opts.icon_size as u32 / 2;
        let y = 80 + row * opts.grid_spacing as u32 + opts.icon_size as u32 / 2;
        records.push(DsRecord::new(
            entry.name.clone(),
            "Iloc",
            DsData::Blob(icon_location_blob(x, y)),
        )?);

        if !entry.is_dir {
            if let Some(ext) = Path::new(&entry.name)
                .extension()
                .and_then(|e| e.to_str())
                .filter(|e| !e.is_empty())
            {
                records.push(DsRecord::new(
                    entry.name.clone(),
                    "extn",
                    DsData::Ustr(ext.to_string()),
                )?);
            }
        } else {
            records.push(DsRecord::new(entry.name.clone(), "vSrn", DsData::Long(1))?);
        }
        records.push(DsRecord::new(
            entry.name.clone(),
            "dilc",
            DsData::Blob(vec![0u8; 32]),
        )?);
    }

    Ok(records)
}

/// Create (or refresh) the `.DS_Store` in `path`.
pub fn write_dsstore(path: &Path, opts: &FinderOptions, skip_hidden: bool) -> Result<PathBuf> {
    let records = make_records(path, opts, skip_hidden)?;
    let store = DsStore::new(records);
    let target = path.join(".DS_Store");
    store.write_path(&target)?;
    Ok(target)
}

/// Recursively shit `.DS_Store` files into every directory, exactly like a
/// Finder window that has visited a tree.
pub fn poop_tree(
    root: &Path,
    opts: &FinderOptions,
    recursive: bool,
    skip_hidden: bool,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    poop_tree_inner(root, opts, recursive, skip_hidden, dry_run, &mut created)?;
    Ok(created)
}

fn poop_tree_inner(
    dir: &Path,
    opts: &FinderOptions,
    recursive: bool,
    skip_hidden: bool,
    dry_run: bool,
    created: &mut Vec<PathBuf>,
) -> Result<()> {
    if !dir.is_dir() {
        return Err(Error::new(format!("{} is not a directory", dir.display())));
    }

    // Children first so a parent store can mention child directories.
    if recursive {
        let mut children: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && (!skip_hidden
                        || p.file_name()
                            .map(|n| n.to_string_lossy().starts_with('.'))
                            .unwrap_or(false))
            })
            .collect();
        children.sort();
        for child in children {
            let meta = fs::symlink_metadata(&child)?;
            if meta.file_type().is_symlink() {
                continue;
            }
            poop_tree_inner(&child, opts, recursive, skip_hidden, dry_run, created)?;
        }
    }

    let target = dir.join(".DS_Store");
    if dry_run {
        created.push(target);
        return Ok(());
    }
    write_dsstore(dir, opts, skip_hidden)?;
    created.push(target);
    Ok(())
}

/// Remove `.DS_Store` files recursively.
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
    let target = dir.join(".DS_Store");
    if target.exists() {
        if dry_run {
            removed.push(target);
        } else {
            fs::remove_file(&target)?;
            removed.push(target);
        }
    }
    if recursive {
        let mut children: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        children.sort();
        for child in children {
            let meta = fs::symlink_metadata(&child)?;
            if !meta.file_type().is_symlink() {
                clean_tree_inner(&child, recursive, dry_run, removed)?;
            }
        }
    }
    Ok(())
}

/// Validate that a serialised `.DS_Store` contains the expected filenames.
#[cfg(test)]
pub(crate) fn contains_names(data: &[u8], names: &[&str]) -> bool {
    match DsStore::parse(data) {
        Ok(parsed) => names
            .iter()
            .all(|n| parsed.records.iter().any(|r| r.filename == *n)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dutc_conversion() {
        // 1970-01-01 -> 2082844800 seconds.
        assert_eq!(unix_to_dutc(0), (2_082_844_800u64) << 16);
        assert_eq!(unix_to_dutc(1), (2_082_844_801u64) << 16);
    }

    #[test]
    fn generated_store_mentions_files() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-finder-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("a.txt"), b"hello").unwrap();
        fs::create_dir(tmp.join("subdir")).unwrap();
        fs::write(tmp.join("subdir/b.txt"), b"world").unwrap();

        let target = write_dsstore(&tmp, &FinderOptions::default(), false).unwrap();
        let bytes = fs::read(&target).unwrap();
        assert!(contains_names(&bytes, &[".", "a.txt", "subdir"]));
        assert!(!contains_names(&bytes, &[".DS_Store"]));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tree_poop() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-tree-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("x/y")).unwrap();
        fs::write(tmp.join("x/a"), b"a").unwrap();
        fs::write(tmp.join("x/y/b"), b"b").unwrap();

        let files = poop_tree(&tmp, &FinderOptions::default(), true, false, false).unwrap();
        assert_eq!(files.len(), 3);
        assert!(tmp.join("x/.DS_Store").exists());
        assert!(tmp.join("x/y/.DS_Store").exists());

        let removed = clean_tree(&tmp, true, false).unwrap();
        assert_eq!(removed.len(), 3);
        let _ = fs::remove_dir_all(&tmp);
    }
}
