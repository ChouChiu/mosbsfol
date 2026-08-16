// SPDX-License-Identifier: Apache-2.0

//! macOS-flavoured extended attributes on top of the maintained
//! [`xattr`] crate.
//!
//! The non-dereferencing variants are used deliberately: they operate on
//! the symlink itself, which matches Finder/`xattr` behaviour on macOS.

pub mod cli;

use std::io;
use std::path::Path;

use crate::shared::bplist::{self, Plist};
use crate::shared::util::{Error, Result};

const ENOTSUP: i32 = 95; // Linux `EOPNOTSUPP`
const ENODATA: i32 = 61; // Linux `ENODATA`

/// Linux requires an xattr namespace.  Bare macOS-style names such as
/// `com.apple.quarantine` are stored in the `user.` namespace and shown
/// without the prefix by this tool.
pub fn kernel_name(name: &str) -> String {
    if name.starts_with("user.")
        || name.starts_with("trusted.")
        || name.starts_with("security.")
        || name.starts_with("system.")
    {
        name.to_string()
    } else {
        format!("user.{name}")
    }
}

pub fn display_name(name: &str) -> String {
    name.strip_prefix("user.")
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

fn xattr_error(what: &str, path: &Path, error: io::Error) -> Error {
    if error.raw_os_error() == Some(ENOTSUP) {
        Error::new(format!(
            "{what} on {}: operation not supported by this filesystem \
             (mount with user_xattr, or use an AppleDouble sidecar)",
            path.display()
        ))
    } else {
        Error::new(format!("{what} on {}: {error}", path.display()))
    }
}

/// List xattr names (NUL-separated by the kernel).
pub fn list(path: &Path) -> Result<Vec<String>> {
    let attrs = xattr::list(path).map_err(|e| xattr_error("listxattr", path, e))?;
    Ok(attrs
        .map(|name| name.to_string_lossy().into_owned())
        .collect())
}

/// Read one xattr, returning `None` when the attribute does not exist.
pub fn get_optional(path: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let what = format!("getxattr {name}");
    xattr::get(path, kernel_name(name)).map_err(|e| xattr_error(&what, path, e))
}

/// Read one xattr as raw bytes.
pub fn get(path: &Path, name: &str) -> Result<Vec<u8>> {
    let what = format!("getxattr {name}");
    get_optional(path, name)?
        .ok_or_else(|| Error::new(format!("{what} on {}: attribute not found", path.display())))
}

/// Set one xattr.
pub fn set(path: &Path, name: &str, value: &[u8]) -> Result<()> {
    let what = format!("setxattr {name}");
    xattr::set(path, kernel_name(name), value).map_err(|e| xattr_error(&what, path, e))
}

/// Remove one xattr.
pub fn remove(path: &Path, name: &str) -> Result<()> {
    let what = format!("removexattr {name}");
    xattr::remove(path, kernel_name(name)).map_err(|e| xattr_error(&what, path, e))
}

/// `com.apple.quarantine` value.  The real format is
/// `flags;timestamp;agent;UUID`; flags `0083` means
/// quarantine + downloaded + "open with warning" is close enough.
pub fn quarantine_value() -> Vec<u8> {
    format!(
        "0083;{};Safari;{}",
        crate::shared::util::unix_now(),
        crate::shared::util::uuid_v4()
    )
    .into_bytes()
}

pub fn set_quarantine(path: &Path) -> Result<()> {
    set(path, "com.apple.quarantine", &quarantine_value())
}

/// `com.apple.FinderInfo`: 32 bytes of FInfo/FXInfo.
pub fn finder_info_value(type_code: &[u8; 4], creator_code: &[u8; 4]) -> Vec<u8> {
    crate::shared::mac::make_finder_info(type_code, creator_code).to_vec()
}

pub fn set_finder_info(path: &Path, type_code: &[u8; 4], creator_code: &[u8; 4]) -> Result<()> {
    write_finder_info(path, &make_finder_info(type_code, creator_code))
}

pub fn make_finder_info(type_code: &[u8; 4], creator_code: &[u8; 4]) -> [u8; 32] {
    crate::shared::mac::make_finder_info(type_code, creator_code)
}

/// Read the 32-byte FinderInfo xattr.  A missing attribute reads as
/// all-zeroes, which is what Finder treats as "no Finder metadata".
pub fn read_finder_info(path: &Path) -> Result<[u8; 32]> {
    match get_optional(path, "com.apple.FinderInfo")? {
        Some(raw) => raw.try_into().map_err(|raw: Vec<u8>| {
            Error::new(format!(
                "com.apple.FinderInfo on {} has {} bytes, expected 32",
                path.display(),
                raw.len()
            ))
        }),
        None => Ok([0u8; 32]),
    }
}

pub fn write_finder_info(path: &Path, info: &[u8; 32]) -> Result<()> {
    set(path, "com.apple.FinderInfo", info)
}

/// Finder label colour bits in `FInfo.fdFlags`.
pub const FINDER_LABEL_MASK: u16 = 0x000e;
/// `kIsInvisible` bit.
pub const FINDER_HIDDEN_MASK: u16 = 0x4000;

fn finder_flags(info: &[u8; 32]) -> u16 {
    u16::from_be_bytes([info[8], info[9]])
}

fn set_finder_flags(info: &mut [u8; 32], flags: u16) {
    info[8..10].copy_from_slice(&flags.to_be_bytes());
}

fn set_label_in_info(info: &mut [u8; 32], label: u16) {
    let flags = (finder_flags(info) & !FINDER_LABEL_MASK) | ((label << 1) & FINDER_LABEL_MASK);
    set_finder_flags(info, flags);
}

fn label_in_info(info: &[u8; 32]) -> u16 {
    (finder_flags(info) & FINDER_LABEL_MASK) >> 1
}

fn set_hidden_in_info(info: &mut [u8; 32], hidden: bool) {
    let flags = finder_flags(info);
    set_finder_flags(
        info,
        if hidden {
            flags | FINDER_HIDDEN_MASK
        } else {
            flags & !FINDER_HIDDEN_MASK
        },
    );
}

fn hidden_in_info(info: &[u8; 32]) -> bool {
    finder_flags(info) & FINDER_HIDDEN_MASK != 0
}

/// Map a Finder colour tag name to its emulated label value (0 = none).
pub fn finder_tag_value(name: &str) -> Result<u16> {
    match name.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(0),
        "gray" | "grey" => Ok(1),
        "green" => Ok(2),
        "purple" => Ok(3),
        "blue" => Ok(4),
        "yellow" => Ok(5),
        "red" => Ok(6),
        "orange" => Ok(7),
        other => Err(Error::new(format!(
            "unknown Finder tag {other:?} (none/gray/green/purple/blue/yellow/red/orange)"
        ))),
    }
}

pub fn finder_tag_name(value: u16) -> &'static str {
    match value & 0x7 {
        0 => "none",
        1 => "gray",
        2 => "green",
        3 => "purple",
        4 => "blue",
        5 => "yellow",
        6 => "red",
        7 => "orange",
        _ => "none",
    }
}

pub fn set_finder_tag(path: &Path, tag: &str) -> Result<()> {
    let mut info = read_finder_info(path)?;
    set_label_in_info(&mut info, finder_tag_value(tag)?);
    write_finder_info(path, &info)
}

pub fn get_finder_tag(path: &Path) -> Result<u16> {
    Ok(label_in_info(&read_finder_info(path)?))
}

pub fn set_hidden(path: &Path, hidden: bool) -> Result<()> {
    let mut info = read_finder_info(path)?;
    set_hidden_in_info(&mut info, hidden);
    write_finder_info(path, &info)
}

pub fn is_hidden(path: &Path) -> Result<bool> {
    Ok(hidden_in_info(&read_finder_info(path)?))
}

/// Raw `com.apple.ResourceFork` xattr access.
pub fn get_resource_fork(path: &Path) -> Result<Vec<u8>> {
    get(path, "com.apple.ResourceFork")
}

pub fn set_resource_fork(path: &Path, data: &[u8]) -> Result<()> {
    set(path, "com.apple.ResourceFork", data)
}

/// Read the Resource Fork for AppleDouble sidecar generation.
///
/// Missing forks and filesystems without xattr support both produce an
/// empty fork; permission errors and other real failures still propagate.
pub fn resource_fork_or_empty(path: &Path) -> Result<Vec<u8>> {
    let kernel = kernel_name("com.apple.ResourceFork");
    match xattr::get(path, kernel) {
        Ok(value) => Ok(value.unwrap_or_default()),
        Err(error) if matches!(error.raw_os_error(), Some(ENOTSUP) | Some(ENODATA)) => {
            Ok(Vec::new())
        }
        Err(error) => Err(xattr_error("getxattr com.apple.ResourceFork", path, error)),
    }
}

/// `com.apple.metadata:kMDItemWhereFroms`: a binary-plist array of URLs.
pub fn where_froms_value(urls: &[String]) -> Result<Vec<u8>> {
    let plist = Plist::Array(urls.iter().cloned().map(Plist::String).collect());
    bplist::encode(&plist)
}

pub fn set_where_froms(path: &Path, urls: &[String]) -> Result<()> {
    set(
        path,
        "com.apple.metadata:kMDItemWhereFroms",
        &where_froms_value(urls)?,
    )
}

/// `com.apple.metadata:kMDItemFinderComment`: a binary-plist string.
pub fn set_finder_comment(path: &Path, comment: &str) -> Result<()> {
    let plist = Plist::String(comment.to_string());
    set(
        path,
        "com.apple.metadata:kMDItemFinderComment",
        &bplist::encode(&plist)?,
    )
}

/// Human-readable FinderInfo summary.
pub fn display_finder_info(raw: &[u8]) -> String {
    if raw.len() != 32 {
        return crate::shared::util::hex_dump(raw, 32);
    }
    let info: &[u8; 32] = raw.try_into().unwrap();
    let type_code = String::from_utf8_lossy(&info[0..4]);
    let creator_code = String::from_utf8_lossy(&info[4..8]);
    let flags = finder_flags(info);
    let mut parts = vec![
        format!("type={type_code:?}"),
        format!("creator={creator_code:?}"),
        format!("tag={}", finder_tag_name(label_in_info(info))),
    ];
    if hidden_in_info(info) {
        parts.push("hidden".to_string());
    }
    parts.push(format!("flags=0x{flags:04x}"));
    parts.join(" ")
}

/// Human-readable display for known macOS xattrs.
pub fn display_value(name: &str, raw: &[u8]) -> String {
    if name.starts_with("com.apple.metadata:") {
        match bplist::decode(raw) {
            Ok(plist) => bplist::to_json(&plist),
            Err(_) => crate::shared::util::hex_dump(raw, 64),
        }
    } else if name == "com.apple.quarantine" {
        String::from_utf8_lossy(raw).into_owned()
    } else if name == "com.apple.FinderInfo" {
        display_finder_info(raw)
    } else {
        let text = String::from_utf8_lossy(raw);
        if text.chars().all(|c| !c.is_control()) && !raw.is_empty() {
            text.into_owned()
        } else {
            crate::shared::util::hex_dump(raw, 64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_roundtrip() {
        assert_eq!(kernel_name("com.apple.x"), "user.com.apple.x");
        assert_eq!(kernel_name("user.foo"), "user.foo");
        assert_eq!(display_name("user.com.apple.x"), "com.apple.x");
        assert_eq!(display_name("security.foo"), "security.foo");
    }

    #[test]
    fn finder_tag_bit_layout() {
        assert_eq!(finder_tag_value("red").unwrap(), 6);
        assert_eq!(finder_tag_name(6), "red");

        let mut info = [0u8; 32];
        set_label_in_info(&mut info, 6);
        assert_eq!(label_in_info(&info), 6);
        assert_eq!(finder_flags(&info), 0x000c);

        set_hidden_in_info(&mut info, true);
        assert!(hidden_in_info(&info));
        set_hidden_in_info(&mut info, false);
        assert!(!hidden_in_info(&info));
        assert_eq!(label_in_info(&info), 6); // hidden bit did not clobber label
    }

    #[test]
    fn finder_info_display() {
        let info = make_finder_info(b"TEXT", b"ttxt");
        let text = display_finder_info(&info);
        assert!(text.contains("TEXT"));
        assert!(text.contains("ttxt"));
        assert!(text.contains("tag=none"));
    }

    #[test]
    fn quarantine_shape() {
        let q = String::from_utf8(quarantine_value()).unwrap();
        let parts: Vec<&str> = q.split(';').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "0083");
        assert_eq!(parts[2], "Safari");
    }

    #[test]
    fn wherefroms_is_bplist() {
        let value = where_froms_value(&["https://example.com/".to_string()]).unwrap();
        assert_eq!(value[..8], *b"bplist00");
        let decoded = crate::shared::bplist::decode(&value).unwrap();
        assert_eq!(
            decoded,
            Plist::Array(vec![Plist::String("https://example.com/".to_string())])
        );
    }

    #[test]
    fn xattr_roundtrip_when_supported() {
        let tmp = std::env::temp_dir().join(format!("mosbsfol-xattr-test-{}", std::process::id()));
        std::fs::write(&tmp, b"x").unwrap();
        match set(&tmp, "user.mosbsfol-test", b"hello") {
            Ok(()) => {
                assert_eq!(get(&tmp, "user.mosbsfol-test").unwrap(), b"hello");
                assert_eq!(get_optional(&tmp, "user.mosbsfol-missing").unwrap(), None);
                remove(&tmp, "user.mosbsfol-test").unwrap();
            }
            Err(error) => {
                // Some containers/tmpfs mounts disable xattrs entirely.
                eprintln!("skipping xattr syscall test: {error}");
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
