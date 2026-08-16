// SPDX-License-Identifier: Apache-2.0

//! Feature `autopoop`: drop the full macOS suite on removable media *and*
//! the local-disk suite on the host machine automatically.
//!
//! There is deliberately no kernel module here: the daemon only needs the
//! kernel interfaces that already exist (`/proc/self/mountinfo` and sysfs),
//! so it stays in safe userspace Rust and can be switched on and off at
//! runtime with a state file.  The matching udev rule calls
//! `mosbsfol autopoop trigger` for an immediate reaction; the built-in
//! polling daemon is the fallback for systems without udev/systemd.
//!
//! Removable media get the full USB treatment (`._*` sidecars, recursive
//! `.DS_Store`, volume traces).  Local fixed disks get the macOS HFS-style
//! treatment: `.DS_Store` plus volume traces, without per-file `._*`
//! sidecars (AppleDouble only exists on filesystems that cannot store
//! forks).  Local roots are non-recursive by default; opt into recursive
//! local `.DS_Store` with `--local-recursive`.  The daemon re-pooops
//! already-known local roots every `--local-rescan` seconds (default one
//! hour) because the host machine has no insertion event to react to.
//!
//! A mount is considered removable when sysfs reports its backing block
//! device as removable.  If sysfs is unavailable the filesystem type is
//! used as a fallback (`vfat`, `exfat`, `ntfs`, ... versus `ext4`, `xfs`,
//! `btrfs`, ...).

pub mod cli;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::features::appledouble;
use crate::features::dsstore::finder::{self, FinderOptions};
use crate::shared::fs as fsx;
use crate::shared::util::{Error, Result};

const DEFAULT_SYSTEM_STATE: &str = "/run/mosbsfol/autopoop/state";
const ENABLED_MARKER: &str = "enabled";

const REMOVABLE_FS_TYPES: &[&str] = &[
    "vfat", "msdos", "exfat", "ntfs", "ntfs3", "fuseblk", "hfs", "hfsplus",
];

const LOCAL_FS_TYPES: &[&str] = &[
    "bcachefs", "btrfs", "ext2", "ext3", "ext4", "f2fs", "jfs", "overlay", "reiserfs", "xfs", "zfs",
];

/// Resolve the autopoop switch state file.
///
/// Precedence: explicit `--state` argument, `MOSBSFOL_AUTOPOOP_STATE`,
/// `$XDG_RUNTIME_DIR/mosbsfol/autopoop/state`, then the system-wide
/// `/run/mosbsfol/autopoop/state` used by the udev rule and systemd unit.
pub fn state_path(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.as_os_str().is_empty()) {
        return path.to_path_buf();
    }
    if let Some(path) = std::env::var_os("MOSBSFOL_AUTOPOOP_STATE").filter(|path| !path.is_empty())
    {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(runtime_dir).join("mosbsfol/autopoop/state");
    }
    PathBuf::from(DEFAULT_SYSTEM_STATE)
}

/// Whether automatic pooping is currently switched on.
///
/// A missing state file means disabled (the safe default).  A file whose
/// trimmed content is `enabled` means enabled; any other content is treated
/// as disabled so a half-written state file can never surprise anyone.
pub fn is_enabled(path: &Path) -> Result<bool> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents.trim() == ENABLED_MARKER),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Switch automatic pooping on.
pub fn enable(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{ENABLED_MARKER}\n"))?;
    Ok(())
}

/// Switch automatic pooping off.
pub fn disable(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// One mounted filesystem entry from `/proc/self/mountinfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub major: u32,
    pub minor: u32,
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub source: String,
}

impl Mount {
    /// Stable identity used by the daemon to notice mounts and re-mounts.
    pub fn device_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.major,
            self.minor,
            self.mount_point.display()
        )
    }

    /// Human-readable `MAJ:MIN` string, matching udev's `%M:%m`.
    pub fn device_number(&self) -> String {
        format!("{}:{}", self.major, self.minor)
    }
}

/// Parse the contents of `/proc/self/mountinfo`.
pub fn parse_mountinfo(contents: &str) -> Result<Vec<Mount>> {
    let mut mounts = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mount = parse_mountinfo_line(line).ok_or_else(|| {
            Error::new(format!(
                "malformed mountinfo entry on line {}: {line:?}",
                index + 1
            ))
        })?;
        mounts.push(mount);
    }
    Ok(mounts)
}

fn parse_mountinfo_line(line: &str) -> Option<Mount> {
    let fields: Vec<&str> = line.split(' ').collect();
    // Field layout:
    //   id parent major:minor root mountpoint options optional... - fs_type source super...
    let separator = fields.iter().position(|field| *field == "-")?;
    if separator < 6 {
        return None;
    }
    let (major, minor) = fields[2].split_once(':')?;
    let major = major.parse::<u32>().ok()?;
    let minor = minor.parse::<u32>().ok()?;
    let mount_point = PathBuf::from(decode_mountinfo_path(fields[4]));
    let fs_type = (*fields.get(separator + 1)?).to_string();
    if fs_type.is_empty() {
        return None;
    }
    let source = fields
        .get(separator + 2)
        .map(|raw| decode_mountinfo_path(raw))
        .unwrap_or_default();
    Some(Mount {
        major,
        minor,
        mount_point,
        fs_type,
        source,
    })
}

/// Decode octal escapes (`\040`, `\011`, `\012`, `\134`) used by mountinfo.
fn decode_mountinfo_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let triplet = &bytes[index + 1..index + 4];
            if triplet.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
                let value =
                    (triplet[0] - b'0') * 64 + (triplet[1] - b'0') * 8 + (triplet[2] - b'0');
                out.push(value);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read the live mount table.
pub fn read_mounts() -> Result<Vec<Mount>> {
    let contents = fs::read_to_string("/proc/self/mountinfo")?;
    parse_mountinfo(&contents)
}

/// Mounts currently attached to removable block devices.
///
/// sysfs is authoritative when available.  Without sysfs the filesystem
/// type fallback keeps the command useful in minimal containers.
pub fn removable_mounts() -> Result<Vec<Mount>> {
    Ok(read_mounts()?
        .into_iter()
        .filter(|mount| mount_kind(mount) == MountKind::Removable)
        .collect())
}

/// Mounts currently attached to local fixed disks (the host machine).
pub fn local_mounts() -> Result<Vec<Mount>> {
    Ok(read_mounts()?
        .into_iter()
        .filter(|mount| mount_kind(mount) == MountKind::Local)
        .collect())
}

/// All current mounts for one block device (`MAJ:MIN`).
pub fn mounts_for_device(major: u32, minor: u32) -> Result<Vec<Mount>> {
    Ok(read_mounts()?
        .into_iter()
        .filter(|mount| mount.major == major && mount.minor == minor)
        .collect())
}

/// Check sysfs for the `removable` attribute of `MAJ:MIN` or one of its
/// parent block devices.  `None` means sysfs could not answer.
pub fn sysfs_device_is_removable(major: u32, minor: u32) -> Option<bool> {
    let link = PathBuf::from(format!("/sys/dev/block/{major}:{minor}"));
    let mut current = fs::canonicalize(link).ok()?;
    loop {
        if let Ok(contents) = fs::read_to_string(current.join("removable")) {
            return Some(contents.trim() == "1");
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Filesystems that only make sense on removable / external media.
pub fn is_removable_fs(fs_type: &str) -> bool {
    REMOVABLE_FS_TYPES.contains(&fs_type)
}

/// Filesystems normally used for local fixed disks.
pub fn is_local_fs(fs_type: &str) -> bool {
    LOCAL_FS_TYPES.contains(&fs_type)
}

/// What autopoop should do with one mounted filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountKind {
    /// USB stick / memory card: full `._*` + `.DS_Store` + volume traces.
    Removable,
    /// Local fixed disk: `.DS_Store` + volume traces, no AppleDouble.
    Local,
    /// Pseudo/virtual filesystem (`proc`, `sysfs`, `tmpfs`, ...): leave it alone.
    Other,
}

/// Classify a mount using sysfs first, filesystem type as fallback.
pub fn mount_kind(mount: &Mount) -> MountKind {
    match sysfs_device_is_removable(mount.major, mount.minor) {
        Some(true) => MountKind::Removable,
        Some(false) => MountKind::Local,
        None if is_removable_fs(&mount.fs_type) => MountKind::Removable,
        None if is_local_fs(&mount.fs_type) => MountKind::Local,
        None => MountKind::Other,
    }
}

/// Parse a `MAJ:MIN` string or a block-device node such as `/dev/sdb1`.
///
/// The udev rule passes `%M:%m`, so that path is the primary format.
/// Device nodes are resolved through `/sys/class/block/<name>/dev`.
pub fn parse_device_spec(spec: &str) -> Result<(u32, u32)> {
    if let Some((major, minor)) = spec.split_once(':') {
        return parse_major_minor(major, minor);
    }

    let name = Path::new(spec)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(spec))
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| Error::new(format!("cannot derive a block device name from {spec:?}")))?;
    let dev_file = Path::new("/sys/class/block").join(&name).join("dev");
    let contents = fs::read_to_string(&dev_file).map_err(|_| {
        Error::new(format!(
            "cannot resolve device {spec:?}: expected MAJ:MIN or an existing /dev block node"
        ))
    })?;
    let (major, minor) = contents
        .trim()
        .split_once(':')
        .ok_or_else(|| Error::new(format!("malformed sysfs dev entry {contents:?}")))?;
    parse_major_minor(major, minor)
}

fn parse_major_minor(major: &str, minor: &str) -> Result<(u32, u32)> {
    let major = major
        .parse::<u32>()
        .map_err(|_| Error::new(format!("invalid device major number {major:?}")))?;
    let minor = minor
        .parse::<u32>()
        .map_err(|_| Error::new(format!("invalid device minor number {minor:?}")))?;
    Ok((major, minor))
}

/// Summary of one automatic-poop run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoopStats {
    pub sidecars: usize,
    pub stores: usize,
    pub volume_traces: usize,
}

impl PoopStats {
    pub fn total(self) -> usize {
        self.sidecars + self.stores + self.volume_traces
    }

    fn summary(self) -> String {
        let mut parts = Vec::new();
        if self.sidecars > 0 {
            parts.push(counted(
                self.sidecars,
                "AppleDouble sidecar",
                "AppleDouble sidecars",
            ));
        }
        if self.stores > 0 {
            parts.push(counted(self.stores, ".DS_Store file", ".DS_Store files"));
        }
        if self.volume_traces > 0 {
            parts.push(counted(self.volume_traces, "volume trace", "volume traces"));
        }
        if parts.is_empty() {
            "nothing to do".to_string()
        } else {
            parts.join(", ")
        }
    }
}

fn counted(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

/// Which droppings a target gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoopStyle {
    /// USB/removable: recursive `._*` + `.DS_Store` + volume traces.
    Removable,
    /// Local disk: `.DS_Store` + volume traces, no `._*` sidecars.
    Local { recursive: bool },
}

/// Drop the automatic suite for one path using an explicit style.
pub fn poop_with_style(path: &Path, style: PoopStyle, dry_run: bool) -> Result<PoopStats> {
    fsx::require_dir(path)?;

    match style {
        PoopStyle::Removable => {
            let sidecars = appledouble::poop_tree_skipping_hidden(path, true, true, true, dry_run)?;
            let stores = finder::poop_tree(path, &FinderOptions::default(), true, true, dry_run)?;
            let volume_traces = poop_volume_if_needed(path, dry_run)?;

            Ok(PoopStats {
                sidecars: sidecars.len(),
                stores: stores.len(),
                volume_traces,
            })
        }
        PoopStyle::Local { recursive } => poop_local_path(path, recursive, dry_run),
    }
}

/// Drop the automatic USB suite on one mount point.
///
/// This is the same full suite as `mosbsfol usb PATH -r --include-dirs
/// --type-codes` (plus `.DS_Store` generation), so it is idempotent enough
/// for udev and daemon duplicates to be harmless.
pub fn poop_path(path: &Path, dry_run: bool) -> Result<PoopStats> {
    poop_with_style(path, PoopStyle::Removable, dry_run)
}

/// Drop the local-disk suite on one path: `.DS_Store` files (recursively if
/// requested) and volume-root traces.  No AppleDouble sidecars are created:
/// local Unix filesystems store forks/metadata natively, and this keeps
/// whole-disk automatic runs from doubling every file count.
pub fn poop_local_path(path: &Path, recursive: bool, dry_run: bool) -> Result<PoopStats> {
    // `skip_hidden` keeps Finder from recursively storing views of `.git`,
    // `.cache`, and the `.Spotlight-V100`/`.fseventsd`/`.Trashes` traces
    // autopoop itself creates on the previous pass.
    let stores = finder::poop_tree(path, &FinderOptions::default(), recursive, true, dry_run)?;
    let volume_traces = poop_volume_if_needed(path, dry_run)?;

    Ok(PoopStats {
        sidecars: 0,
        stores: stores.len(),
        volume_traces,
    })
}

/// Volume traces are only dropped once per autopoop target.  Re-pooping
/// already-pooped roots would otherwise accumulate a new Spotlight
/// `Store-V2/<uuid>` directory and a new `.fseventsd` UUID on every udev,
/// daemon, and hourly-local pass.
fn poop_volume_if_needed(path: &Path, dry_run: bool) -> Result<usize> {
    #[cfg(feature = "volumetrace")]
    {
        if path.join(".Spotlight-V100").exists() {
            return Ok(0);
        }
        Ok(crate::features::volumetrace::poop_volume(path, dry_run)?.len())
    }
    #[cfg(not(feature = "volumetrace"))]
    {
        let _ = (path, dry_run);
        Ok(0)
    }
}

fn print_mount_result(path: &Path, stats: PoopStats, dry_run: bool) {
    if dry_run {
        println!("👃 would create {} in {}", stats.summary(), path.display());
    } else {
        println!("💩 {} in {}", stats.summary(), path.display());
        println!("🪰 {} now smells like a Mac.", path.display());
    }
}

/// Behaviour knobs shared by `once`, `run` and `trigger`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutopoopOptions {
    /// Also poop local fixed-disk mounts (not just removable media).
    pub include_local: bool,
    /// Recurse into local mount roots when generating `.DS_Store`.
    pub local_recursive: bool,
    /// How often the daemon re-pooops already-known local mount roots.
    pub local_rescan_secs: u64,
}

impl Default for AutopoopOptions {
    fn default() -> Self {
        Self {
            include_local: true,
            local_recursive: false,
            local_rescan_secs: 3600,
        }
    }
}

#[derive(Clone, Debug)]
struct WatchTarget {
    key: String,
    path: PathBuf,
    style: PoopStyle,
}

fn watch_targets(options: AutopoopOptions) -> Result<Vec<WatchTarget>> {
    let mut targets = Vec::new();
    for mount in removable_mounts()? {
        targets.push(WatchTarget {
            key: format!("usb:{}", mount.device_key()),
            path: mount.mount_point,
            style: PoopStyle::Removable,
        });
    }
    if options.include_local {
        for mount in local_mounts()? {
            targets.push(WatchTarget {
                key: format!("local:{}", mount.device_key()),
                path: mount.mount_point,
                style: PoopStyle::Local {
                    recursive: options.local_recursive,
                },
            });
        }
    }
    Ok(targets)
}

fn local_targets(recursive: bool) -> Result<Vec<WatchTarget>> {
    Ok(local_mounts()?
        .into_iter()
        .map(|mount| WatchTarget {
            key: format!("local:{}", mount.device_key()),
            path: mount.mount_point,
            style: PoopStyle::Local { recursive },
        })
        .collect())
}

fn poop_target(target: &WatchTarget, dry_run: bool) -> Result<PoopStats> {
    poop_with_style(&target.path, target.style, dry_run)
}

fn print_disabled_hint(state: &Path) {
    println!(
        "⏸️ autopoop is disabled (state file {}); run `mosbsfol autopoop enable` or pass --force",
        state.display()
    );
}

fn process_targets(targets: &[WatchTarget], dry_run: bool) {
    for target in targets {
        match poop_target(target, dry_run) {
            Ok(stats) => print_mount_result(&target.path, stats, dry_run),
            Err(error) => eprintln!("⚠️ autopoop failed for {}: {error}", target.path.display()),
        }
    }
}

/// One manual pass over removable media plus (by default) local fixed disks.
pub fn run_once(
    explicit_path: Option<&Path>,
    force: bool,
    state: &Path,
    dry_run: bool,
    options: AutopoopOptions,
) -> Result<()> {
    if !force && !is_enabled(state)? {
        print_disabled_hint(state);
        return Ok(());
    }

    if let Some(path) = explicit_path {
        let stats = poop_path(path, dry_run)?;
        print_mount_result(path, stats, dry_run);
        return Ok(());
    }

    let targets = watch_targets(options)?;
    if targets.is_empty() {
        println!("👃 no mounted removable or local filesystems found");
        return Ok(());
    }
    process_targets(&targets, dry_run);
    Ok(())
}

/// One manual local-machine pass: an explicit directory, or every local
/// fixed-disk mount currently visible.
pub fn run_local_once(
    explicit_path: Option<&Path>,
    recursive: bool,
    force: bool,
    state: &Path,
    dry_run: bool,
) -> Result<()> {
    if !force && !is_enabled(state)? {
        print_disabled_hint(state);
        return Ok(());
    }

    if let Some(path) = explicit_path {
        let stats = poop_local_path(path, recursive, dry_run)?;
        print_mount_result(path, stats, dry_run);
        return Ok(());
    }

    let targets = local_targets(recursive)?;
    if targets.is_empty() {
        println!("👃 no mounted local filesystems found");
        return Ok(());
    }
    process_targets(&targets, dry_run);
    Ok(())
}

/// Handle one udev `add` event for a block device.
///
/// Removable devices are always handled.  Local fixed disks are only
/// handled when `include_local` is set; the udev rule leaves that off and
/// lets the daemon cover the host machine.
pub fn run_trigger(
    device: &str,
    force: bool,
    state: &Path,
    dry_run: bool,
    include_local: bool,
    local_recursive: bool,
) -> Result<()> {
    if !force && !is_enabled(state)? {
        return Ok(());
    }

    let (major, minor) = parse_device_spec(device)?;
    let mounts = mounts_for_device(major, minor)?;
    if mounts.is_empty() {
        println!("👃 device {major}:{minor} has no mounted filesystem yet");
        return Ok(());
    }

    let mut handled = 0;
    for mount in &mounts {
        let style = match mount_kind(mount) {
            MountKind::Removable => Some(PoopStyle::Removable),
            MountKind::Local if include_local => Some(PoopStyle::Local {
                recursive: local_recursive,
            }),
            MountKind::Local => {
                println!(
                    "🪑 skipping local mount {} ({}); pass --include-local to poop local disks",
                    mount.mount_point.display(),
                    mount.source
                );
                None
            }
            MountKind::Other => {
                println!(
                    "🪑 skipping non-disk mount {} ({}), fs {}",
                    mount.mount_point.display(),
                    mount.source,
                    mount.fs_type
                );
                None
            }
        };
        let Some(style) = style else { continue };

        match poop_with_style(&mount.mount_point, style, dry_run) {
            Ok(stats) => {
                print_mount_result(&mount.mount_point, stats, dry_run);
                handled += 1;
            }
            Err(error) => eprintln!(
                "⚠️ autopoop failed for {}: {error}",
                mount.mount_point.display()
            ),
        }
    }
    if handled == 0 {
        println!("👃 device {major}:{minor} has no pooped filesystem mount");
    }
    Ok(())
}

/// Run the polling daemon until interrupted.
///
/// The daemon watches removable media and (by default) local fixed disks.
/// It keeps a `known` set while disabled too, so toggling the switch on
/// only affects filesystems that appear after the switch is flipped.
pub fn run_daemon(
    interval_secs: u64,
    state: &Path,
    dry_run: bool,
    options: AutopoopOptions,
) -> Result<()> {
    println!(
        "👃 autopoop daemon watching removable media and {} local disks (poll {interval_secs}s, local rescan {}s, state {})",
        if options.include_local { "all" } else { "no" },
        options.local_rescan_secs,
        state.display()
    );
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut last_local_pass: Option<Instant> = None;
    let mut was_enabled = is_enabled(state)?;
    println!(
        "autopoop is {}",
        if was_enabled { "enabled" } else { "disabled" }
    );

    loop {
        let now = Instant::now();
        let enabled = is_enabled(state)?;
        let just_enabled = enabled && !was_enabled;
        if enabled != was_enabled {
            println!(
                "autopoop is now {}",
                if enabled { "enabled" } else { "disabled" }
            );
            was_enabled = enabled;
        }

        let targets = watch_targets(options)?;
        let current: BTreeSet<String> = targets.iter().map(|target| target.key.clone()).collect();
        let mut next_known = current.clone();
        let mut handled: BTreeSet<String> = BTreeSet::new();

        if enabled {
            // New mounts (removable or local) get their first pass.
            for target in targets.iter().filter(|target| !known.contains(&target.key)) {
                handled.insert(target.key.clone());
                match poop_target(target, dry_run) {
                    Ok(stats) => print_mount_result(&target.path, stats, dry_run),
                    Err(error) => {
                        eprintln!("⚠️ autopoop failed for {}: {error}", target.path.display());
                        // Removable media often show up before the
                        // automounter is fully done, so retry those.
                        // Local fixed disks usually fail for a durable
                        // reason (read-only mount, permissions), so do not
                        // retry them every polling interval.
                        if target.style == PoopStyle::Removable {
                            next_known.remove(&target.key);
                        }
                    }
                }
            }

            // The host machine is not event-driven, so re-poop already
            // known local mount roots every `local_rescan_secs`.  Re-enabling
            // the switch also triggers an immediate local pass.
            let local_due = options.include_local
                && (just_enabled
                    || last_local_pass.is_none_or(|last| {
                        now.duration_since(last) >= Duration::from_secs(options.local_rescan_secs)
                    }));
            if local_due {
                for target in targets.iter().filter(|target| {
                    matches!(target.style, PoopStyle::Local { .. })
                        && !handled.contains(&target.key)
                }) {
                    match poop_target(target, dry_run) {
                        Ok(stats) => print_mount_result(&target.path, stats, dry_run),
                        Err(error) => {
                            eprintln!("⚠️ autopoop failed for {}: {error}", target.path.display());
                        }
                    }
                }
                last_local_pass = Some(now);
            } else if matches!(
                targets.iter().find(|target| handled.contains(&target.key)),
                Some(WatchTarget {
                    style: PoopStyle::Local { .. },
                    ..
                })
            ) {
                last_local_pass = Some(now);
            }
        }

        known = next_known;
        thread::sleep(Duration::from_secs(interval_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MOUNTINFO: &str = "\
29 24 8:17 / /mnt/usb\\040stick rw,noatime shared:11 - vfat /dev/sdb1 rw
30 24 259:2 / / ro,noatime master:1 - btrfs /dev/nvme0n1p2 rw
31 25 8:18 /sub /media/exfat rw,nosuid - exfat /dev/sdb2 rw
";

    #[test]
    fn parses_mountinfo_and_unescapes_paths() {
        let mounts = parse_mountinfo(SAMPLE_MOUNTINFO).unwrap();
        assert_eq!(mounts.len(), 3);
        assert_eq!(mounts[0].major, 8);
        assert_eq!(mounts[0].minor, 17);
        assert_eq!(mounts[0].mount_point, PathBuf::from("/mnt/usb stick"));
        assert_eq!(mounts[0].fs_type, "vfat");
        assert_eq!(mounts[0].source, "/dev/sdb1");
        assert_eq!(mounts[0].device_key(), "8:17:/mnt/usb stick");
    }

    #[test]
    fn rejects_malformed_mountinfo() {
        assert!(parse_mountinfo("hello world").is_err());
        assert!(parse_mountinfo("1 1 nope / / rw - vfat /dev/sdb1 rw").is_err());
    }

    #[test]
    fn state_switch_roundtrip() {
        let state = std::env::temp_dir().join(format!(
            "mosbsfol-autopoop-state-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&state);

        assert!(!is_enabled(&state).unwrap());
        enable(&state).unwrap();
        assert!(is_enabled(&state).unwrap());
        assert_eq!(fs::read_to_string(&state).unwrap().trim(), ENABLED_MARKER);

        disable(&state).unwrap();
        assert!(!state.exists());
        assert!(!is_enabled(&state).unwrap());
        disable(&state).unwrap();
    }

    #[test]
    fn parses_device_specs() {
        assert_eq!(parse_device_spec("8:17").unwrap(), (8, 17));
        assert_eq!(parse_device_spec("259:0").unwrap(), (259, 0));
        assert!(parse_device_spec("banana").is_err());
    }

    #[test]
    fn removable_fs_fallback_is_restrictive() {
        assert!(is_removable_fs("vfat"));
        assert!(is_removable_fs("exfat"));
        assert!(!is_removable_fs("btrfs"));
        assert!(!is_removable_fs("ext4"));
    }

    #[test]
    fn local_fs_fallback_is_restrictive() {
        assert!(is_local_fs("btrfs"));
        assert!(is_local_fs("ext4"));
        assert!(is_local_fs("overlay"));
        assert!(!is_local_fs("vfat"));
        assert!(!is_local_fs("tmpfs"));
    }

    #[test]
    fn mount_kind_falls_back_to_filesystem_type() {
        let mount = |fs_type: &str| Mount {
            major: 424_242,
            minor: 1,
            mount_point: PathBuf::from("/mnt/test"),
            fs_type: fs_type.to_string(),
            source: "/dev/test".to_string(),
        };
        assert_eq!(mount_kind(&mount("vfat")), MountKind::Removable);
        assert_eq!(mount_kind(&mount("ext4")), MountKind::Local);
        assert_eq!(mount_kind(&mount("overlay")), MountKind::Local);
        assert_eq!(mount_kind(&mount("tmpfs")), MountKind::Other);
    }

    #[test]
    fn poop_path_drops_the_usb_suite() {
        let dir = std::env::temp_dir().join(format!(
            "mosbsfol-autopoop-path-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("hello.txt"), b"hi").unwrap();

        let stats = poop_path(&dir, false).unwrap();
        assert_eq!(stats.stores, 1);
        assert_eq!(stats.sidecars, 1);
        assert!(dir.join(".DS_Store").is_file());
        assert!(dir.join("._hello.txt").is_file());
        assert!(stats.total() >= 2);

        // A second automatic pass must be idempotent: no new Spotlight
        // UUIDs and no sidecars / .DS_Store files inside the droppings the
        // first pass created.
        let second = poop_path(&dir, false).unwrap();
        assert_eq!(second.sidecars, 1);
        assert_eq!(second.stores, 1);
        assert!(!dir.join("._Icon\r").exists());
        assert!(!dir.join(".Spotlight-V100/.DS_Store").exists());
        assert!(!dir.join(".Spotlight-V100/._Store-V2").exists());
        #[cfg(feature = "volumetrace")]
        {
            assert_eq!(second.volume_traces, 0);
            assert_eq!(
                fs::read_dir(dir.join(".Spotlight-V100/Store-V2"))
                    .unwrap()
                    .count(),
                1
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_poop_skips_sidecars_and_can_recurse() {
        let dir = std::env::temp_dir().join(format!(
            "mosbsfol-autopoop-local-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("hello.txt"), b"hi").unwrap();
        fs::write(dir.join("sub/world.txt"), b"yo").unwrap();

        let stats = poop_local_path(&dir, false, false).unwrap();
        assert_eq!(stats.sidecars, 0);
        assert_eq!(stats.stores, 1);
        assert!(dir.join(".DS_Store").is_file());
        assert!(!dir.join("._hello.txt").exists());
        assert!(!dir.join("sub/.DS_Store").exists());

        let stats = poop_local_path(&dir, true, false).unwrap();
        assert_eq!(stats.sidecars, 0);
        assert_eq!(stats.stores, 2);
        assert!(dir.join("sub/.DS_Store").is_file());
        assert!(!dir.join("sub/._world.txt").exists());

        #[cfg(feature = "volumetrace")]
        {
            let first_store_count = fs::read_dir(dir.join(".Spotlight-V100/Store-V2"))
                .unwrap()
                .count();
            let stats = poop_local_path(&dir, false, false).unwrap();
            assert_eq!(stats.volume_traces, 0);
            let second_store_count = fs::read_dir(dir.join(".Spotlight-V100/Store-V2"))
                .unwrap()
                .count();
            assert_eq!(first_store_count, second_store_count);
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
