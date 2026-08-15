// SPDX-License-Identifier: Apache-2.0

//! Tiny Macintosh data-layout helpers shared by several features.

/// 32-byte `FInfo` + `FXInfo` record.  `type_code` and `creator_code` are
/// Macintosh FourCCs; pass `"????"` for "none".
pub fn make_finder_info(type_code: &[u8; 4], creator_code: &[u8; 4]) -> [u8; 32] {
    let mut info = [0u8; 32];
    info[0..4].copy_from_slice(type_code);
    info[4..8].copy_from_slice(creator_code);
    info
}

/// Best-effort Macintosh file type FourCC for a file name/extension.
pub fn mac_type_for_name(name: &str) -> [u8; 4] {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let code = match ext.as_str() {
        "txt" | "md" | "log" => b"TEXT",
        "jpg" | "jpeg" => b"JPEG",
        "png" => b"PNGf",
        "gif" => b"GIFf",
        "pdf" => b"PDF ",
        "html" | "htm" => b"TEXT",
        "mp3" => b"MP3 ",
        "mp4" => b"MPG4",
        "zip" => b"ZIP ",
        _ => b"????",
    };
    *code
}

/// Names that are macOS droppings rather than user files.
pub fn is_macos_volume_marker(name: &str) -> bool {
    name == ".DS_Store"
        || name.starts_with("._")
        || matches!(
            name,
            ".localized"
                | ".VolumeIcon.icns"
                | ".Spotlight-V100"
                | ".fseventsd"
                | ".Trashes"
                | ".TemporaryItems"
                | ".DocumentRevisions-V100"
                | "__MACOSX"
        )
        || name == "Icon\r"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_info_layout() {
        let info = make_finder_info(b"TEXT", b"ttxt");
        assert_eq!(&info[0..4], b"TEXT");
        assert_eq!(&info[4..8], b"ttxt");
        assert!(info[8..].iter().all(|b| *b == 0));
    }
}
