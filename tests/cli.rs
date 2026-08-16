// SPDX-License-Identifier: Apache-2.0

//! End-to-end CLI smoke tests.
#![cfg(any(
    feature = "dsstore",
    feature = "maczip",
    feature = "plist",
    feature = "volumetrace",
    feature = "autopoop"
))]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mosbsfol"))
}

#[test]
fn clap_help_and_version_work() {
    let out = bin().arg("--help").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Usage: mosbsfol"));
    assert!(text.contains("--version"));

    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("mosbsfol "));
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

#[cfg(feature = "dsstore")]
#[test]
fn dsstore_skip_hidden_does_not_touch_hidden_subtrees() {
    let dir = tmp("dsstore-hidden");
    fs::write(dir.join("a.txt"), b"a").unwrap();
    fs::create_dir_all(dir.join(".hidden/sub")).unwrap();

    let out = bin()
        .args(["poop", dir.to_str().unwrap(), "-r", "--skip-hidden"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join(".DS_Store").is_file());
    assert!(!dir.join(".hidden/.DS_Store").exists());
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

#[cfg(feature = "autopoop")]
#[test]
fn autopoop_switch_toggles_with_state_file() {
    let dir = tmp("autopoop-state");
    let state = dir.join("state");
    let state = state.to_str().unwrap();

    let out = bin()
        .args(["autopoop", "status", "--state", state])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("DISABLED"));

    let out = bin()
        .args(["autopoop", "enable", "--state", state])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = bin()
        .args(["autopoop", "status", "--state", state])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("ENABLED"));

    let out = bin()
        .args(["autopoop", "disable", "--state", state])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!std::path::Path::new(state).exists());
}

#[cfg(feature = "autopoop")]
#[test]
fn autopoop_once_force_poops_path() {
    let dir = tmp("autopoop-once");
    let state = dir.join("state");
    fs::write(dir.join("file.txt"), b"x").unwrap();

    let out = bin()
        .args([
            "autopoop",
            "once",
            dir.to_str().unwrap(),
            "--state",
            state.to_str().unwrap(),
            "--force",
        ])
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

#[cfg(feature = "autopoop")]
#[test]
fn autopoop_once_obeys_disabled_switch() {
    let dir = tmp("autopoop-disabled");
    let state = dir.join("state");
    fs::write(dir.join("file.txt"), b"x").unwrap();

    let out = bin()
        .args([
            "autopoop",
            "once",
            dir.to_str().unwrap(),
            "--state",
            state.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!dir.join(".DS_Store").exists());
    assert!(!dir.join("._file.txt").exists());
}

#[cfg(feature = "autopoop")]
#[test]
fn autopoop_local_poops_dsstore_without_sidecars() {
    let dir = tmp("autopoop-local");
    let state = dir.join("state");
    fs::write(dir.join("file.txt"), b"x").unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/deep.txt"), b"x").unwrap();

    let out = bin()
        .args([
            "autopoop",
            "local",
            dir.to_str().unwrap(),
            "--force",
            "--state",
            state.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join(".DS_Store").is_file());
    assert!(!dir.join("._file.txt").exists());
    assert!(!dir.join("sub/.DS_Store").exists());

    let out = bin()
        .args([
            "autopoop",
            "local",
            dir.to_str().unwrap(),
            "-r",
            "--force",
            "--state",
            state.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(dir.join("sub/.DS_Store").is_file());
    assert!(!dir.join("sub/._deep.txt").exists());
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
