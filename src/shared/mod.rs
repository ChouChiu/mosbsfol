// SPDX-License-Identifier: Apache-2.0

//! Shared infrastructure: no macOS behaviour of its own, but used by
//! several features.
//!
//! * `bplist`: thin `bplist00` helpers over the `plist` crate
//! * `cli`: shared [`clap`] argument builders
//! * `fs`: shared directory/symlink traversal helpers
//! * `mac`: small Macintosh data layouts (e.g. FInfo/FXInfo)
//! * `util`: UTF-16BE, FourCC, alignment, error type

#[cfg(any(feature = "dsstore", feature = "plist", feature = "xattr"))]
pub mod bplist;
pub(crate) mod cli;
pub(crate) mod fs;
pub mod mac;
pub mod util;
