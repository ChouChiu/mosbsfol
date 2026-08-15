// SPDX-License-Identifier: Apache-2.0

//! End-to-end CLI smoke tests.
#![cfg(any(
    feature = "dsstore",
    feature = "maczip",
    feature = "plist",
    feature = "volumetrace"
))]

//! End-to-end CLI smoke tests.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mosbsfol"))
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mosbsfol-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[cfg(feature = "dsstore")]
#[test]
fn dsstore_poop_and_inspect() {
    let dir = tmp("dsstore");
    fs::write(dir.join("a.txt"), b"a").unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/b.txt"), b"b").unwrap();

    let out = bin()
        .args(["dsstore", "poop", dir.to_str().unwrap(), "-r"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join(".DS_Store").is_file());
    assert!(dir.join("sub/.DS_Store").is_file());

    let out = bin()
        .args([
            "dsstore",
            "inspect",
            dir.join(".DS_Store").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("a.txt"));
    assert!(text.contains("sub"));
    assert!(text.contains("bwsp"));
}

#[cfg(all(feature = "appledouble", feature = "dsstore"))]
#[test]
fn usb_creates_macos_droppings() {
    let dir = tmp("usb");
    fs::write(dir.join("file.txt"), b"x").unwrap();

    let out = bin()
        .args(["usb", dir.to_str().unwrap(), "--type-codes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join(".DS_Store").is_file());
    let sidecar = dir.join("._file.txt");
    assert!(sidecar.is_file());
    let data = fs::read(&sidecar).unwrap();
    assert_eq!(&data[0..4], &[0x00, 0x05, 0x16, 0x07]);
}

#[cfg(feature = "plist")]
#[test]
fn plist_binary_and_xml_roundtrip() {
    let dir = tmp("plist");
    let file = dir.join("x.plist");
    let out = bin()
        .args([
            "plist",
            "write",
            file.to_str().unwrap(),
            "name=mosbsfol",
            "answer=42",
            "enabled=true",
            "pi=3.14",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(&fs::read(&file).unwrap()[..8], b"bplist00");

    let out = bin()
        .args(["plist", "read", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"answer\":42"));
    assert!(text.contains("\"name\":\"mosbsfol\""));
}

#[cfg(feature = "volumetrace")]
#[test]
fn trace_creates_volume_markers() {
    let dir = tmp("trace");
    let out = bin()
        .args(["trace", "poop", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join(".Spotlight-V100").is_dir());
    assert!(dir.join(".fseventsd/fseventsd-uuid").is_file());
    assert!(dir.join(".Trashes").is_dir());
    assert!(dir.join(".TemporaryItems").is_dir());
    assert!(dir.join(".localized").is_file());
    assert!(dir.join(".VolumeIcon.icns").is_file());
}

#[cfg(feature = "maczip")]
#[test]
fn maczip_contains_macosx_sidecars() {
    let dir = tmp("maczip");
    fs::write(dir.join("a.txt"), b"a").unwrap();
    let out = bin()
        .args([
            "maczip",
            dir.to_str().unwrap(),
            dir.join("out.zip").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("__MACOSX/._a.txt"));
}
