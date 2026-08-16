// SPDX-License-Identifier: Apache-2.0

//! Shared infrastructure: no macOS behaviour of its own, but used by
//! several features.
//!
//! * `bplist`: thin `bplist00` helpers over the `plist` crate
//! * `cli`: shared [`clap`] argument builders
//! * `mac`: small Macintosh data layouts (e.g. FInfo/FXInfo)
//! * `util`: UTF-16BE, FourCC, alignment, error type

pub mod bplist;
pub(crate) mod cli;
pub mod mac;
pub mod util;
