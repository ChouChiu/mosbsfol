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
use std::time::UNIX_EPOCH;

use super::format::{DsData, DsRecord, DsStore};
use crate::shared::bplist::{self, Plist};
use crate::shared::fs::{self as fsx, file_name};
use crate::shared::util::{Error, Result};

/// Seconds between the 1904 Mac epoch and the 1970 Unix epoch.
pub const MAC_EPOCH_DELTA: i64 = 2_082_844_800;

const ICON_COLUMNS: usize = 12;
const ICON_LEFT: u32 = 40;
const ICON_TOP: u32 = 80;
const DILC_BLOB_SIZE: usize = 32;

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

impl FinderOptions {
    fn validate(&self) -> Result<()> {
        if self.view_style.len() != 4 || !self.view_style.is_ascii() {
            return Err(Error::new(format!(
                "view style {:?} must be a four-character ASCII code",
                self.view_style
            )));
        }
        Ok(())
    }
}

/// Unix seconds -> Mac `dutc` ticks (1/65536 s since 1904-01-01).
pub fn unix_to_dutc(unix_seconds: i64) -> u64 {
    ((unix_seconds.saturating_add(MAC_EPOCH_DELTA)) as u64).saturating_mul(65536)
}

fn modified_dutc(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| unix_to_dutc(duration.as_secs() as i64))
        .unwrap_or_else(|| unix_to_dutc(crate::shared::util::unix_now() as i64))
}

fn metadata_modified_dutc(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| unix_to_dutc(duration.as_secs() as i64))
        .unwrap_or_else(|| unix_to_dutc(crate::shared::util::unix_now() as i64))
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
        // `DirEntry::metadata` does not follow a final symlink, matching how
        // Finder treats the link itself as the visible item.
        let meta = entry.metadata()?;
        let is_dir = meta.is_dir();
        out.push(VisibleEntry {
            name,
            is_dir,
            size: meta.len(),
            modified_dutc: metadata_modified_dutc(&meta),
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
    let mut blob = Vec::with_capacity(16);
    blob.extend_from_slice(&x.to_be_bytes());
    blob.extend_from_slice(&y.to_be_bytes());
    blob.extend_from_slice(&[0xff; 6]);
    blob.extend_from_slice(&[0, 0]);
    blob
}

fn dict(entries: impl IntoIterator<Item = (&'static str, Plist)>) -> Plist {
    Plist::Dictionary(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn window_plist(opts: &FinderOptions) -> Plist {
    dict([
        ("WindowBounds", Plist::String(opts.window_bounds.clone())),
        ("SidebarWidth", Plist::Real(180.0)),
        ("ShowSidebar", Plist::Boolean(true)),
        ("ShowToolbar", Plist::Boolean(false)),
        ("ShowStatusBar", Plist::Boolean(true)),
        ("ShowPathbar", Plist::Boolean(false)),
        ("ViewStyle", Plist::String(opts.view_style.clone())),
    ])
}

fn icon_view_plist(opts: &FinderOptions) -> Plist {
    dict([
        ("viewOptionsVersion", Plist::Integer(1.into())),
        ("iconSize", Plist::Integer((opts.icon_size as i64).into())),
        ("textSize", Plist::Integer((opts.text_size as i64).into())),
        ("arrangeBy", Plist::String("none".to_string())),
        ("showIconPreview", Plist::Boolean(true)),
        ("showItemInfo", Plist::Boolean(false)),
        ("labelOnBottom", Plist::Boolean(true)),
        (
            "gridSpacing",
            Plist::Integer((opts.grid_spacing as i64).into()),
        ),
        ("scrollPositionX", Plist::Integer(0.into())),
        ("scrollPositionY", Plist::Integer(0.into())),
        ("gridOffsetX", Plist::Integer(0.into())),
        ("gridOffsetY", Plist::Integer(0.into())),
        ("backgroundColorRed", Plist::Real(1.0)),
        ("backgroundColorGreen", Plist::Real(1.0)),
        ("backgroundColorBlue", Plist::Real(1.0)),
    ])
}

fn legacy_icvo(opts: &FinderOptions) -> Vec<u8> {
    let mut blob = Vec::with_capacity(32);
    blob.extend_from_slice(b"icv4");
    blob.extend_from_slice(&opts.icon_size.to_be_bytes());
    blob.extend_from_slice(b"none");
    blob.extend_from_slice(b"botm");
    blob.extend_from_slice(&[0u8; 12]);
    blob
}

fn fwi0_blob(opts: &FinderOptions) -> Vec<u8> {
    let mut blob = Vec::with_capacity(16);
    for value in [44u16, 100, 600, 1020] {
        blob.extend_from_slice(&value.to_be_bytes());
    }
    blob.extend_from_slice(opts.view_style.as_bytes());
    blob.extend_from_slice(&[0u8; 4]);
    blob
}

fn default_background_blob() -> Vec<u8> {
    let mut blob = Vec::with_capacity(12);
    blob.extend_from_slice(b"DefB");
    blob.extend_from_slice(&[0u8; 8]);
    blob
}

/// Build Finder records for one directory.
pub fn make_records(path: &Path, opts: &FinderOptions, skip_hidden: bool) -> Result<Vec<DsRecord>> {
    opts.validate()?;
    let entries = visible_entries(path, skip_hidden)?;
    let logical_size: u64 = entries.iter().map(|entry| entry.size).sum();
    let physical_size = logical_size.saturating_add(8191) & !8191u64;
    let now_dutc = modified_dutc(path);

    let mut records = Vec::with_capacity(14 + entries.len() * 3);
    records.extend([
        DsRecord::new(".", "vstl", DsData::Type(opts.view_style.clone()))?,
        DsRecord::new(".", "icvo", DsData::Blob(legacy_icvo(opts)))?,
        DsRecord::new(".", "icvt", DsData::Shor(opts.text_size))?,
        DsRecord::new(
            ".",
            "icvp",
            DsData::Blob(bplist::encode(&icon_view_plist(opts))?),
        )?,
        DsRecord::new(
            ".",
            "bwsp",
            DsData::Blob(bplist::encode(&window_plist(opts))?),
        )?,
        DsRecord::new(".", "fwi0", DsData::Blob(fwi0_blob(opts)))?,
        DsRecord::new(".", "fwsw", DsData::Long(180))?,
        DsRecord::new(".", "fwvh", DsData::Shor(600))?,
        DsRecord::new(".", "BKGD", DsData::Blob(default_background_blob()))?,
        DsRecord::new(".", "ICVO", DsData::Bool(true))?,
        DsRecord::new(".", "vSrn", DsData::Long(1))?,
        DsRecord::new(".", "logS", DsData::Comp(logical_size as i64))?,
        DsRecord::new(".", "phyS", DsData::Comp(physical_size as i64))?,
        DsRecord::new(".", "modD", DsData::Dutc(now_dutc))?,
    ]);

    // Place icons on a Finder-ish grid.
    for (index, entry) in entries.iter().enumerate() {
        let col = (index % ICON_COLUMNS) as u32;
        let row = (index / ICON_COLUMNS) as u32;
        let x = ICON_LEFT + col * opts.grid_spacing as u32 + opts.icon_size as u32 / 2;
        let y = ICON_TOP + row * opts.grid_spacing as u32 + opts.icon_size as u32 / 2;
        records.push(DsRecord::new(
            entry.name.clone(),
            "Iloc",
            DsData::Blob(icon_location_blob(x, y)),
        )?);

        if entry.is_dir {
            records.push(DsRecord::new(entry.name.clone(), "vSrn", DsData::Long(1))?);
        } else if let Some(ext) = Path::new(&entry.name)
            .extension()
            .and_then(|ext| ext.to_str())
            .filter(|ext| !ext.is_empty())
        {
            records.push(DsRecord::new(
                entry.name.clone(),
                "extn",
                DsData::Ustr(ext.to_string()),
            )?);
        }
        records.push(DsRecord::new(
            entry.name.clone(),
            "dilc",
            DsData::Blob(vec![0u8; DILC_BLOB_SIZE]),
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
    fsx::require_dir(dir)?;

    if recursive {
        for child in child_directories(dir, skip_hidden)? {
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

/// Sorted child directories.  Symlinks are never followed and hidden
/// directories are excluded when `skip_hidden` is set.
fn child_directories(dir: &Path, skip_hidden: bool) -> Result<Vec<PathBuf>> {
    let mut children = Vec::new();
    for path in fsx::sorted_dir_entries(dir)? {
        let name = file_name(&path);
        if skip_hidden && name.starts_with('.') {
            continue;
        }
        let meta = fsx::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            children.push(path);
        }
    }
    Ok(children)
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
    if fsx::exists_no_follow(&target)? {
        if dry_run {
            removed.push(target);
        } else {
            fs::remove_file(&target)?;
            removed.push(target);
        }
    }

    if recursive {
        for child in child_directories(dir, false)? {
            clean_tree_inner(&child, recursive, dry_run, removed)?;
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
            .all(|name| parsed.records.iter().any(|record| record.filename == *name)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dutc_conversion() {
        // 1970-01-01 -> 2082844800 seconds.
        assert_eq!(unix_to_dutc(0), 2_082_844_800u64 << 16);
        assert_eq!(unix_to_dutc(1), 2_082_844_801u64 << 16);
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
    fn tree_poop_and_clean() {
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

    #[test]
    fn skip_hidden_does_not_recurse_into_dotdirs() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-hidden-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("visible/sub")).unwrap();
        fs::create_dir_all(tmp.join(".hidden/deep")).unwrap();
        fs::write(tmp.join(".hidden/deep/file"), b"x").unwrap();

        let _files = poop_tree(&tmp, &FinderOptions::default(), true, true, false).unwrap();
        assert!(tmp.join(".DS_Store").exists());
        assert!(tmp.join("visible/.DS_Store").exists());
        assert!(!tmp.join(".hidden/.DS_Store").exists());

        // Without `skip_hidden`, the hidden tree is processed as well.
        let files = poop_tree(&tmp, &FinderOptions::default(), true, false, false).unwrap();
        assert!(tmp.join(".hidden/.DS_Store").exists());
        assert_eq!(files.len(), 5);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn invalid_view_style_is_rejected() {
        let opts = FinderOptions {
            view_style: "not-a-fourcc".to_string(),
            ..FinderOptions::default()
        };
        let tmp = std::env::temp_dir().join(format!("mosbsfol-invalid-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        assert!(make_records(&tmp, &opts, false).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }
}
