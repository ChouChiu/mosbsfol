// SPDX-License-Identifier: Apache-2.0

//! Small argument helpers shared by the CLI commands in each feature.

use crate::shared::util::{Error, Result};

#[allow(dead_code)]
pub(crate) fn need(args: &[String], i: usize, what: &str) -> Result<String> {
    args.get(i)
        .map(|s| s.to_string())
        .ok_or_else(|| Error::new(format!("missing {what}")))
}

#[allow(dead_code)]
pub(crate) fn has_flag(args: &[String], names: &[&str]) -> bool {
    args.iter().any(|a| names.contains(&a.as_str()))
}

#[allow(dead_code)]
pub(crate) fn first_positional(args: &[String], skip_flags: bool) -> Option<String> {
    for a in args {
        if a.starts_with("--") {
            continue;
        }
        if skip_flags && a.starts_with('-') && a.len() > 1 {
            continue;
        }
        return Some(a.to_string());
    }
    None
}

#[allow(dead_code)]
pub(crate) fn positionals_after(args: &[String], skip: usize) -> Vec<String> {
    args.iter()
        .skip(skip)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .collect()
}
