// SPDX-License-Identifier: Apache-2.0

//! Finder-style stored (method 0) ZIP writing, backed by the [`zip`]
//! crate rather than a hand-rolled archive layout.

use std::io::{Cursor, Write};

use ::zip::write::SimpleFileOptions;
use ::zip::{CompressionMethod, DateTime, ZipWriter};

use crate::shared::util::{Error, Result};

#[derive(Clone, Debug)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

fn zip_error(e: ::zip::result::ZipError) -> Error {
    Error::new(e.to_string())
}

/// Serialise a stored ZIP containing `entries`.
pub fn write_zip(entries: &[ZipEntry]) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default_for_write())
        .large_file(true);

    for entry in entries {
        writer
            .start_file(&entry.name, options.unix_permissions(entry.mode & 0xffff))
            .map_err(zip_error)?;
        writer.write_all(&entry.data)?;
    }

    Ok(writer.finish().map_err(zip_error)?.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn zip_signatures_and_roundtrip() {
        let bytes = write_zip(&[ZipEntry {
            name: "hello.txt".into(),
            data: b"hello".to_vec(),
            mode: 0o100644,
        }])
        .unwrap();
        assert_eq!(&bytes[0..4], &0x0403_4b50u32.to_le_bytes());

        let mut archive = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 1);
        let mut file = archive.by_name("hello.txt").unwrap();
        let mut data = String::new();
        file.read_to_string(&mut data).unwrap();
        assert_eq!(data, "hello");
    }
}
