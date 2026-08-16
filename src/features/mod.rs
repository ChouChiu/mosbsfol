// SPDX-License-Identifier: Apache-2.0

//! Feature modules.
//!
//! Feature-Driven layout: every directory below `src/features` is one
//! user-visible behaviour (or, for `autopoop`, automation around those
//! behaviours) and is compiled only when the matching Cargo feature is
//! enabled.

#[cfg(feature = "autopoop")]
pub mod autopoop;

#[cfg(feature = "appledouble")]
pub mod appledouble;

#[cfg(feature = "dsstore")]
pub mod dsstore;

#[cfg(feature = "maczip")]
pub mod maczip;

#[cfg(feature = "plist")]
pub mod plist;

#[cfg(feature = "volumetrace")]
pub mod volumetrace;

#[cfg(feature = "xattr")]
pub mod xattr;
