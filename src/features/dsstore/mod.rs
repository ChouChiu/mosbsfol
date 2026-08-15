// SPDX-License-Identifier: Apache-2.0

//! Feature `dsstore`: Finder `.DS_Store` reading/writing and generation.
//!
//! * `format`: binary file-format implementation
//! * `finder`: Finder-record generation and recursive tree operations

pub mod cli;
pub mod finder;
pub mod format;

pub use format::{display_record, display_value, records_sorted_cmp, DsData, DsRecord, DsStore};
