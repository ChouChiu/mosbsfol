// SPDX-License-Identifier: Apache-2.0

//! Shared infrastructure: no macOS behaviour of its own, but used by
//! several features.
//!
//! * `bplist`: binary property-list codec
//! * `cli`: command-line argument helpers
//! * `mac`: small Macintosh data layouts (e.g. FInfo/FXInfo)
//! * `util`: UTF-16BE, FourCC, alignment, error type

pub mod bplist;
pub(crate) mod cli;
pub mod mac;
pub mod util;
