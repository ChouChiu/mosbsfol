// SPDX-License-Identifier: Apache-2.0

//! MOSBSFOL - macOS Bull Shit Feature On Linux.
//!
//! Feature-Driven Rust implementation of six thoroughly non-essential
//! macOS behaviours plus an `autopoop` automation layer.  Common file
//! formats and OS APIs are delegated to
//! maintained crates (`clap`, `plist`, `zip`, `xattr`, `base64`, `uuid`) so the repository itself focuses on the macOS-specific logic:
//!
//! * feature `dsstore`: valid binary `.DS_Store` files
//! * feature `appledouble`: AppleDouble `._*` USB sidecars
//! * feature `maczip`: `__MACOSX/` entries in Finder-style ZIPs
//! * feature `plist`: XML and `bplist00` property lists
//! * feature `xattr`: macOS-style extended attributes
//! * feature `volumetrace`: `.Spotlight-V100`, `.fseventsd`, `.Trashes`,
//!   `.TemporaryItems`, `.localized`, `.VolumeIcon.icns`, `Icon\r`
//! * feature `autopoop`: daemon + udev trigger that drop the USB suite on
//!   removable media and the local-disk suite on the host automatically,
//!   with a runtime on/off state file
//!
//! Layout:
//! * [`core`] - application bootstrap and feature-gated CLI dispatch
//! * [`shared`] - infrastructure shared by several features
//! * [`features`] - one directory per feature
//!
//! All seven features are enabled by default.  Build a subset with:
//!
//! ```sh
//! cargo build --no-default-features --features plist,xattr
//! ```

pub mod core;
pub mod features;
pub mod shared;

pub use shared::{mac, util};

#[cfg(any(feature = "dsstore", feature = "plist", feature = "xattr"))]
pub use shared::bplist;

#[cfg(feature = "autopoop")]
pub use features::autopoop;

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
