// SPDX-License-Identifier: Apache-2.0

//! Minimal stored (method 0) ZIP writer.
//!
//! Enough to reproduce the Finder's `__MACOSX/` sidecar layout without
//! adding a compression dependency.

use std::io::Write;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
const VERSION: u16 = 20;
const UTF8_FLAG: u16 = 0x0800;

pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

pub fn write_zip(entries: &[ZipEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::with_capacity(entries.len());

    for entry in entries {
        let crc = crc32(&entry.data);
        let (dos_time, dos_date) = dos_datetime(crate::shared::util::unix_now());
        let name = entry.name.as_bytes();
        let offset = out.len() as u32;

        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&UTF8_FLAG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&dos_time.to_le_bytes());
        out.extend_from_slice(&dos_date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.write_all(name).unwrap();
        out.write_all(&entry.data).unwrap();
        offsets.push(offset);

        central.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
        central.extend_from_slice(&(3 << 8 | VERSION).to_le_bytes()); // UNIX
        central.extend_from_slice(&VERSION.to_le_bytes());
        central.extend_from_slice(&UTF8_FLAG.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&dos_time.to_le_bytes());
        central.extend_from_slice(&dos_date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra len
        central.extend_from_slice(&0u16.to_le_bytes()); // comment len
        central.extend_from_slice(&0u16.to_le_bytes()); // disk start
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&((entry.mode & 0xffff) << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.write_all(name).unwrap();
    }

    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn dos_datetime(unix_secs: u64) -> (u16, u16) {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let year = year.clamp(1980, 2107) as u16;
    let date = ((year - 1980) << 9) | ((month as u16) << 5) | day as u16;
    let time = (((rem / 3600) as u16) << 11)
        | ((((rem % 3600) / 60) as u16) << 5)
        | (((rem % 60) / 2) as u16);
    (time, date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn zip_signatures_and_sizes() {
        let bytes = write_zip(&[ZipEntry {
            name: "hello.txt".into(),
            data: b"hello".to_vec(),
            mode: 0o100644,
        }]);
        assert_eq!(&bytes[0..4], &LOCAL_SIG.to_le_bytes());
        assert!(bytes.windows(4).any(|w| w == EOCD_SIG.to_le_bytes()));
    }
}
