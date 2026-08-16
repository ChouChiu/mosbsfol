# Repository Guidelines

## Project Overview

`mosbsfol` (MOSBSFOL — "macOS Bull Shit Feature On Linux") is a Rust CLI that recreates six macOS filesystem behaviors on Linux for compatibility testing: `.DS_Store` Finder metadata, AppleDouble `._*` sidecars, `__MACOSX/` Finder-style ZIPs, XML/`bplist00` property lists, `com.apple.*` extended attributes, and volume-root droppings (`.Spotlight-V100`, `.fseventsd`, `.Trashes`, …). Every behavior is a Cargo feature, all enabled by default. Common formats and OS APIs are delegated to mature crates (`clap`, `plist`, `zip`, `xattr`, `base64`, `uuid`); the macOS-specific logic lives in this repo.

## Architecture & Data Flow

Feature-Driven Design: `core` = application bootstrap/CLI dispatch, `shared` = infrastructure used by multiple features, `features/{name}` = one directory per user-visible behavior, each compiled only when its Cargo feature is enabled.

```
main.rs (collect OsString args losslessly)
  └─> core::cli::run(&[OsString])
        ├─ root_command()   // clap Command "mosbsfol", subcommand_required
        │    └─ appends per-feature subcommands inside #[cfg(feature = "...")] blocks
        └─ dispatch(name, matches)
             └─ features::<feature>::cli::execute(&ArgMatches) -> Result<()>
                  └─ writes files (sidecars, .DS_Store, ZIPs, xattrs, volume markers)
```

- `src/lib.rs` is the crate root: `pub mod core/features/shared` plus feature-gated re-exports of each feature module.
- Each feature dir has `mod.rs` (implementation + `pub mod cli;`) and `cli.rs` exposing `command() -> clap::Command` and `execute(&ArgMatches) -> Result<()>`.
- Two command idioms coexist: subcommand-based (`dsstore`, `plist`, `xattr`, `volumetrace`) and flag-driven single-command (`maczip`, `usb`).
- `appledouble/cli.rs` is additionally gated `#[cfg(all(feature = "appledouble", feature = "dsstore"))]` because the `usb` command composes both (plus optional `volumetrace`).

## Key Directories

| Path | Purpose |
| --- | --- |
| `src/` | Crate root (`lib.rs` library + `main.rs` binary bootstrap) |
| `src/core/` | CLI bootstrap: `cli.rs` builds the root clap command and feature-gated dispatch |
| `src/shared/` | Cross-feature infrastructure: `util.rs` (Error/helpers), `fs.rs` (traversal/symlinks), `cli.rs` (clap arg builders), `mac.rs` (FInfo/FXInfo, type codes, trace detection), `bplist.rs` (plist wrapper) |
| `src/features/` | One directory per macOS behavior, gated in `mod.rs` via `#[cfg(feature = "...")] pub mod <name>;` |
| `src/features/dsstore/` | `.DS_Store` binary format (Bud1 + buddy allocator + B-tree), Finder record generation |
| `tests/` | Integration tests (`cli.rs`) |
| `scripts/` | `check-features.sh` (feature-matrix check), `acceptance.sh` (end-to-end validation) |

## Development Commands

```sh
cargo build --release                     # full binary
cargo build --no-default-features --features dsstore   # feature subset
cargo run -- usb /mnt/usb -r --include-dirs --type-codes
cargo install --path .                    # install to ~/.cargo/bin

cargo fmt --check                         # formatting (no rustfmt.toml; defaults)
cargo clippy --all-targets -- -D warnings # lint (no clippy.toml; defaults)
cargo test                                # unit + integration tests

./scripts/check-features.sh               # 64-combination `cargo check` + sampled `cargo test`
./scripts/acceptance.sh                   # end-to-end acceptance (builds release, validates output)
```

## Code Conventions & Common Patterns

- **File header**: every `.rs` file starts with `// SPDX-License-Identifier: Apache-2.0`, then a `//!` module doc. Items use `///` doc comments.
- **Error handling**: no `anyhow`/`thiserror`. A single custom `Error` enum in `src/shared/util.rs` with variants `Message(String)` and `Io(std::io::Error)`; construct via `Error::new(format!(...))`, `io::Error` passes through via `From`. Return `Result<()>` (alias of `std::result::Result<T, Error>`). The binary maps `Err` to `ExitCode::FAILURE` and prints `mosbsfol: error: {e}`.
- **Safety**: `unsafe_code = "forbid"` in `Cargo.toml`; the entire codebase is safe Rust.
- **Feature gating**: every feature-specific item is `#[cfg(feature = "...")]`. Optional dependencies are declared with `dep:<crate>` so `--no-default-features` builds with only `clap`.
- **CLI contract**: a feature's `cli.rs` exposes `command()` (clap `Command` builder) and `execute(&ArgMatches) -> Result<()>`; `execute` matches `Some(("name" | "alias", m))` and returns `Error::new(...)` for unknown/missing subcommands. Core only composes; features own their command + execution.
- **Argument builders**: reuse `src/shared/cli.rs` (`path_arg`, `recursive_flag`, `dry_run_flag`, `flag`, `optional_path`, `required_path`) instead of hand-rolling clap args.
- **Naming**: `poop` = generate droppings, `clean` = remove; `poop_tree`/`clean_tree`/`poop_volume`/`clean_volume`; build-then-write split (`make_records`, `build_maczip`) enables `--dry-run`. Emoji output markers: `💩`/`🗜️`/`🪰`.
- **Self-reference inside a feature**: `use super as <feature>;` then `<feature>::<fn>(...)`.
- **State management**: no global state or DI framework; pure functions plus plain structs (`DsStore`, `FinderOptions`, `MacZipPlan`) carrying options/data. Temp output paths use `std::env::temp_dir()` + `std::process::id()` suffixes in tests.

## Important Files

| File | Role |
| --- | --- |
| `src/main.rs` | Binary entry: collects `OsString` args, calls `core::cli::run`, maps result to `ExitCode` |
| `src/lib.rs` | Library entry + feature-gated re-exports (`pub use dsstore::{DsData, DsRecord, DsStore}` etc.) |
| `src/core/cli.rs` | Root clap command assembly and feature-gated dispatch (`run`, `dispatch`, `root_command`) |
| `src/shared/util.rs` | `Error` enum, `Result` alias, FourCC/UTF-16BE/hex/alignment helpers |
| `src/shared/mac.rs` | `make_finder_info` (32-byte FInfo+FXInfo), `mac_type_for_name`, `is_macos_volume_marker` |
| `Cargo.toml` | Feature graph, optional deps, `[[bin]]`, release profile, `unsafe_code = "forbid"` |
| `tests/cli.rs` | Feature-gated integration suite driving the built binary |
| `scripts/check-features.sh` | Exhaustive feature-combination `cargo check` + sampled `cargo test` |
| `scripts/acceptance.sh` | End-to-end validation via `xxd`/`grep`/Python `zipfile` |

## Runtime/Tooling Preferences

- **Runtime/toolchain**: Rust, **edition 2021**, stable toolchain. No MSRV pinned (no `rust-version` field).
- **Package manager**: Cargo (single crate; no workspace). `Cargo.lock` committed.
- **Tooling constraints**: no CI (no `.github/`), no `rustfmt.toml`/`clippy.toml` — use rustfmt/clippy defaults. `clap` is the only non-optional dependency (`default-features = false`).
- **Release profile**: `strip = true`, `lto = true`, `codegen-units = 1`.
- **License**: Apache-2.0 (`LICENSE` + `NOTICE`, "Copyright 2026 ChouChiu").
- **Build output**: binary `mosbsfol` (`[[bin]]` → `src/main.rs`).

## Testing & QA

- **Framework**: Rust built-in `#[test]` only — no `assert_cmd`/`tempfile`/`proptest` dev-dependencies. Integration tests invoke the compiled binary via `std::process::Command` using `env!("CARGO_BIN_EXE_mosbsfol")`.
- **Unit tests**: inline `#[cfg(test)] mod tests` in every module (temp-dir integration-style tests included).
- **Integration tests** (`tests/cli.rs`): file-level gate `#![cfg(any(feature = "dsstore", feature = "maczip", feature = "plist", feature = "volumetrace"))]` — note `appledouble` and `xattr` are **not** in that list, so the file is empty for those features. Each test is independently `cfg`-gated. Tests assert `status.success()` plus stdout substring / magic-byte / file-existence checks via a `tmp()` helper.
- **QA scripts**: `scripts/acceptance.sh` builds `--release` and validates real output (magic bytes via `xxd`, text via `grep -q`, ZIP entries via a Python `zipfile` heredoc) with `set -euo pipefail` and `pass()`/`fail()` helpers; the xattr section self-skips (`[SKIP]`) when the filesystem lacks `user` xattr support rather than failing.
- **Coverage expectation**: all 64 feature combinations must `cargo check` clean (`scripts/check-features.sh`, no always-exit-0 trap — failures propagate as non-zero exit). Cross-validation notes in README reference Python `plistlib`/`zipfile`/independent DS_Store parsers; no coverage metric is enforced.
