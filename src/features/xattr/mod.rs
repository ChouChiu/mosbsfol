// SPDX-License-Identifier: Apache-2.0

//! Thin wrapper around the Linux `l*` extended-attribute syscalls, plus
//! generators for the macOS `com.apple.*` namespace.
//!
//! The `l` variants are used deliberately: they operate on the symlink
//! itself, which matches Finder/`xattr` behaviour on macOS.

pub mod cli;

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::shared::bplist::{self, Plist};
use crate::shared::util::{Error, Result};

#[cfg(unix)]
#[link(name = "c")]
unsafe extern "C" {
    fn llistxattr(path: *const std::ffi::c_char, list: *mut std::ffi::c_char, size: usize)
        -> isize;
    fn lgetxattr(
        path: *const std::ffi::c_char,
        name: *const std::ffi::c_char,
        value: *mut std::ffi::c_void,
        size: usize,
    ) -> isize;
    fn lsetxattr(
        path: *const std::ffi::c_char,
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_void,
        size: usize,
        flags: std::ffi::c_int,
    ) -> std::ffi::c_int;
    fn lremovexattr(
        path: *const std::ffi::c_char,
        name: *const std::ffi::c_char,
    ) -> std::ffi::c_int;
}

fn path_c(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::new("path contains an interior NUL byte"))
}

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
        .map(|s| s.to_string())
        .unwrap_or_else(|| name.to_string())
}

fn name_c(name: &str) -> Result<CString> {
    let kernel = kernel_name(name);
    CString::new(kernel.as_bytes()).map_err(|_| Error::new("xattr name contains NUL"))
}

fn last_os_error(what: &str, path: &Path) -> Error {
    let e = std::io::Error::last_os_error();
    let kind = e.raw_os_error().unwrap_or(0);
    if kind == 95 {
        Error::new(format!(
            "{} on {}: operation not supported by this filesystem \
             (mount with user_xattr, or use an AppleDouble sidecar)",
            what,
            path.display()
        ))
    } else {
        Error::new(format!("{} on {}: {e}", what, path.display()))
    }
}

/// List xattr names (NUL-separated by the kernel).
pub fn list(path: &Path) -> Result<Vec<String>> {
    let p = path_c(path)?;
    unsafe {
        let len = llistxattr(p.as_ptr(), std::ptr::null_mut(), 0);
        if len < 0 {
            return Err(last_os_error("listxattr", path));
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; len as usize];
        let got = llistxattr(p.as_ptr(), buf.as_mut_ptr().cast(), buf.len());
        if got < 0 {
            return Err(last_os_error("listxattr", path));
        }
        buf.truncate(got as usize);
        Ok(buf
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect())
    }
}

/// Read one xattr as raw bytes.
pub fn get(path: &Path, name: &str) -> Result<Vec<u8>> {
    let p = path_c(path)?;
    let n = name_c(name)?;
    unsafe {
        let len = lgetxattr(p.as_ptr(), n.as_ptr(), std::ptr::null_mut(), 0);
        if len < 0 {
            return Err(last_os_error(&format!("getxattr {name}"), path));
        }
        let mut buf = vec![0u8; len as usize];
        let got = lgetxattr(p.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len());
        if got < 0 {
            return Err(last_os_error(&format!("getxattr {name}"), path));
        }
        buf.truncate(got as usize);
        Ok(buf)
    }
}

/// Set one xattr.
pub fn set(path: &Path, name: &str, value: &[u8]) -> Result<()> {
    let p = path_c(path)?;
    let n = name_c(name)?;
    unsafe {
        if lsetxattr(
            p.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        ) != 0
        {
            return Err(last_os_error(&format!("setxattr {name}"), path));
        }
    }
    Ok(())
}

/// Remove one xattr.
pub fn remove(path: &Path, name: &str) -> Result<()> {
    let p = path_c(path)?;
    let n = name_c(name)?;
    unsafe {
        if lremovexattr(p.as_ptr(), n.as_ptr()) != 0 {
            return Err(last_os_error(&format!("removexattr {name}"), path));
        }
    }
    Ok(())
}

fn pseudo_uuid() -> String {
    let now = crate::shared::util::unix_now();
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_micros())
        .unwrap_or(0);
    format!(
        "{now:08x}-{micros:04x}-4{pid:03x}-9{seq:03x}-{time:012x}",
        pid = std::process::id() & 0xfff,
        seq = micros & 0xfff,
        time = now
    )
}

/// `com.apple.quarantine` value.  The real format is
/// `flags;timestamp;agent;UUID`; flags `0083` means
/// quarantine + downloaded + "open with warning" is close enough.
pub fn quarantine_value() -> Vec<u8> {
    format!(
        "0083;{};Safari;{}",
        crate::shared::util::unix_now(),
        pseudo_uuid()
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
    write_finder_info(
        path,
        &crate::shared::mac::make_finder_info(type_code, creator_code),
    )
}

/// Read the 32-byte FinderInfo xattr, or all-zeroes when absent.
pub fn read_finder_info(path: &Path) -> [u8; 32] {
    get(path, "com.apple.FinderInfo")
        .ok()
        .and_then(|raw| raw.try_into().ok())
        .unwrap_or([0u8; 32])
}

pub fn write_finder_info(path: &Path, info: &[u8; 32]) -> Result<()> {
    set(path, "com.apple.FinderInfo", info)
}

/// Finder label colour bits in `FInfo.fdFlags`.
pub const FINDER_LABEL_MASK: u16 = 0x000e;
/// `kIsInvisible` bit.
pub const FINDER_HIDDEN_MASK: u16 = 0x4000;

/// Map a Finder colour tag name to its emulated label value (0 = none).
pub fn finder_tag_value(name: &str) -> Result<u16> {
    let value = match name.trim().to_ascii_lowercase().as_str() {
        "" | "none" => 0,
        "gray" | "grey" => 1,
        "green" => 2,
        "purple" => 3,
        "blue" => 4,
        "yellow" => 5,
        "red" => 6,
        "orange" => 7,
        other => {
            return Err(Error::new(format!(
                "unknown Finder tag {other:?} (none/gray/green/purple/blue/yellow/red/orange)"
            )));
        }
    };
    Ok(value)
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
    let tag_value = finder_tag_value(tag)?;
    let mut info = read_finder_info(path);
    let flags = u16::from_be_bytes([info[8], info[9]]);
    let flags = (flags & !FINDER_LABEL_MASK) | ((tag_value << 1) & FINDER_LABEL_MASK);
    info[8..10].copy_from_slice(&flags.to_be_bytes());
    write_finder_info(path, &info)
}

pub fn get_finder_tag(path: &Path) -> u16 {
    let info = read_finder_info(path);
    let flags = u16::from_be_bytes([info[8], info[9]]);
    (flags & FINDER_LABEL_MASK) >> 1
}

pub fn set_hidden(path: &Path, hidden: bool) -> Result<()> {
    let mut info = read_finder_info(path);
    let flags = u16::from_be_bytes([info[8], info[9]]);
    let flags = if hidden {
        flags | FINDER_HIDDEN_MASK
    } else {
        flags & !FINDER_HIDDEN_MASK
    };
    info[8..10].copy_from_slice(&flags.to_be_bytes());
    write_finder_info(path, &info)
}

pub fn is_hidden(path: &Path) -> bool {
    let info = read_finder_info(path);
    let flags = u16::from_be_bytes([info[8], info[9]]);
    flags & FINDER_HIDDEN_MASK != 0
}

/// Raw `com.apple.ResourceFork` xattr access.
pub fn get_resource_fork(path: &Path) -> Result<Vec<u8>> {
    get(path, "com.apple.ResourceFork")
}

pub fn set_resource_fork(path: &Path, data: &[u8]) -> Result<()> {
    set(path, "com.apple.ResourceFork", data)
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
        crate::shared::util::hex_dump(raw, 32)
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
    fn finder_tag_bit_layout() {
        assert_eq!(finder_tag_value("red").unwrap(), 6);
        assert_eq!(finder_tag_name(6), "red");
        let tmp = std::env::temp_dir().join(format!("mosbsfol-tag-test-{}", std::process::id()));
        std::fs::write(&tmp, b"x").unwrap();
        // Pure bit manipulation test; syscall side is optional.
        set_finder_tag(&tmp, "red").ok();
        assert_eq!(get_finder_tag(&tmp), 6);
        set_hidden(&tmp, true).ok();
        assert!(is_hidden(&tmp));
        set_hidden(&tmp, false).ok();
        let _ = std::fs::remove_file(&tmp);
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
        assert_eq!(&value[..8], b"bplist00");
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
                remove(&tmp, "user.mosbsfol-test").unwrap();
            }
            Err(e) => {
                // Some containers/tmpfs mounts disable xattrs entirely.
                eprintln!("skipping xattr syscall test: {e}");
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
