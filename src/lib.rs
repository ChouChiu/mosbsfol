// SPDX-License-Identifier: Apache-2.0

//! MOSBSFOL - macOS Bull Shit Feature On Linux.
//!
//! Feature-Driven Rust implementation of six thoroughly non-essential
//! macOS behaviours.  Common file formats and OS APIs are delegated to
//! maintained crates (`clap`, `plist`, `zip`, `xattr`, `base64`, `uuid`, `libc`, `thiserror`, `anyhow`) so the
//! repository itself focuses on the macOS-specific logic:
//!
//! * feature `dsstore`: valid binary `.DS_Store` files
//! * feature `appledouble`: AppleDouble `._*` USB sidecars
//! * feature `maczip`: `__MACOSX/` entries in Finder-style ZIPs
//! * feature `plist`: XML and `bplist00` property lists
//! * feature `xattr`: macOS-style extended attributes
//! * feature `volumetrace`: `.Spotlight-V100`, `.fseventsd`, `.Trashes`,
//!   `.TemporaryItems`, `.localized`, `.VolumeIcon.icns`, `Icon\r`
//!
//! Layout:
//! * [`core`] - application bootstrap and feature-gated CLI dispatch
//! * [`shared`] - infrastructure shared by several features
//! * [`features`] - one directory per feature
//!
//! All six features are enabled by default.  Build a subset with:
//!
//! ```sh
//! cargo build --no-default-features --features plist,xattr
//! ```

pub mod core;
pub mod features;
pub mod shared;

pub use shared::{bplist, mac, util};

#[cfg(feature = "appledouble")]
pub use features::appledouble;

#[cfg(feature = "dsstore")]
pub use features::dsstore::{finder, format as dsstore};

#[cfg(feature = "maczip")]
pub use features::maczip;

#[cfg(feature = "plist")]
pub use features::plist;

#[cfg(feature = "volumetrace")]
pub use features::volumetrace;

#[cfg(feature = "xattr")]
pub use features::xattr;

#[cfg(feature = "dsstore")]
pub use dsstore::{DsData, DsRecord, DsStore};
