//! Character avatars: small square pictures kept as files in the app data
//! directory's `avatars/` folder and served to the webview through the
//! `avatar://` protocol (see `serve`).
//!
//! Files rather than database blobs: `list_characters` runs every time a tab
//! comes back, and the session loads its character on every connect —
//! neither needs the picture, and dragging tens of kilobytes per character
//! along with every row would be for nothing.
//!
//! A character's `avatar_path` holds the file's bare name, never a path, and
//! every name is minted here: a fresh UUID plus the extension of the format
//! the bytes actually are. So a value not shaped like one — from a
//! hand-edited database or backup — is simply not an avatar, and nothing
//! outside the folder can be reached through it.
//!
//! Changing a picture always writes a new file under a new name instead of
//! overwriting the old one, so the webview can never show a cached copy of
//! the previous image under a name it already knows. The file left behind is
//! removed when its character lets go of it, and `sweep` catches whatever
//! that misses — a picture chosen in an editor that was then closed without
//! saving, say.

pub mod generate;

use std::collections::HashSet;
use std::path::Path;

use base64::Engine;
use tauri::http::{Response, StatusCode, header};

pub const DIR_NAME: &str = "avatars";

/// Well above anything the app produces itself — the editor crops to a
/// 512 px square — with room for a picture carried in from a backup.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
}

impl Format {
    /// Decided by the bytes, never by what the caller claims they are.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else {
            None
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "png" => Some(Self::Png),
            "jpg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

/// Whether `name` is one `save` could have minted: a lowercase hyphenated
/// UUID and one of the known extensions. Strict on purpose — it is what
/// keeps a stored name from being a path, and `Uuid::parse_str` alone would
/// also take forms like `urn:uuid:…`, which on Windows names an alternate
/// data stream rather than a file.
pub fn is_valid_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    is_minted_stem(stem) && Format::from_ext(ext).is_some()
}

/// A lowercase hyphenated UUID and nothing else: the stem of every file name
/// this app mints, here and for voice samples (`voice::sample`).
pub(crate) fn is_minted_stem(stem: &str) -> bool {
    stem.len() == 36
        && stem
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-')
        && uuid::Uuid::parse_str(stem).is_ok()
}

/// Stores a picture and returns the name to put in `avatar_path`.
pub fn save(dir: &Path, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err(crate::tr!(
            format!(
                "That image is too large for an avatar (the limit is {} MB)",
                MAX_BYTES / 1_048_576
            ),
            format!("图片太大，头像不能超过 {} MB", MAX_BYTES / 1_048_576),
        ));
    }
    let format = Format::sniff(bytes).ok_or_else(|| {
        crate::tr!(
            "Avatars must be PNG, JPEG or WebP images",
            "头像只支持 PNG、JPEG 或 WebP 图片",
        )
        .to_string()
    })?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = format!("{}.{}", uuid::Uuid::new_v4(), format.ext());
    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
    Ok(name)
}

pub fn read(dir: &Path, name: &str) -> Option<(Vec<u8>, Format)> {
    if !is_valid_name(name) {
        return None;
    }
    let bytes = std::fs::read(dir.join(name)).ok()?;
    let format = Format::sniff(&bytes)?;
    Some((bytes, format))
}

pub fn exists(dir: &Path, name: &str) -> bool {
    is_valid_name(name) && dir.join(name).is_file()
}

/// Deletes a picture nothing points at any more. Failing to is not worth
/// failing the edit that let go of it over: `sweep` gets another go at the
/// next launch.
pub fn remove(dir: &Path, name: &str) {
    if !is_valid_name(name) {
        return;
    }
    match std::fs::remove_file(dir.join(name)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("failed to remove avatar {name}: {e}"),
    }
}

/// Removes every picture in `dir` that isn't in `keep`. Only files named the
/// way `save` names them are touched, so anything else that ends up in the
/// folder is left alone.
///
/// Runs at startup, before any window exists — later on, a picture just
/// chosen in the editor but not yet saved would look exactly like an orphan.
pub fn sweep(dir: &Path, keep: &HashSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if is_valid_name(name) && !keep.contains(name) && std::fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

pub fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode(data: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| e.to_string())
}

pub fn to_data_url(bytes: &[u8], format: Format) -> String {
    format!("data:{};base64,{}", format.mime(), encode(bytes))
}

/// The bytes of a `data:…;base64,…` URL, as the editor's cropper hands them
/// over. The declared type is ignored; `save` looks at the bytes.
pub fn decode_data_url(url: &str) -> Result<Vec<u8>, String> {
    let malformed = || crate::tr!("Not an image data URL", "不是有效的图片数据").to_string();
    let (meta, payload) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .ok_or_else(malformed)?;
    if !meta.ends_with(";base64") {
        return Err(malformed());
    }
    decode(payload)
}

/// Answers an `avatar://localhost/<name>` request (`http://avatar.localhost/<name>`
/// on Windows). Anything that isn't a stored picture is a plain 404, which
/// the webview's `<img>` turns into its fallback.
pub fn serve(dir: Option<&Path>, path: &str) -> Response<Vec<u8>> {
    let name = path.trim_start_matches('/');
    let response = match dir.and_then(|dir| read(dir, name)) {
        Some((bytes, format)) => Response::builder()
            .header(header::CONTENT_TYPE, format.mime())
            // Names are never reused for different content (see the module
            // docs), so a copy the webview already holds is always right.
            .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
            .body(bytes),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new()),
    };
    response.unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 ";

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("voicechat-avatar-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tells_formats_apart_by_their_bytes() {
        assert_eq!(Format::sniff(PNG), Some(Format::Png));
        assert_eq!(Format::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Format::Jpeg));
        assert_eq!(Format::sniff(WEBP), Some(Format::Webp));
        assert_eq!(Format::sniff(b"GIF89a"), None);
        assert_eq!(Format::sniff(b"<svg"), None);
        assert_eq!(Format::sniff(b""), None);
    }

    #[test]
    fn only_minted_names_count() {
        assert!(is_valid_name("0f8fad5b-d9cb-469f-a165-70867728950e.png"));
        assert!(is_valid_name("0f8fad5b-d9cb-469f-a165-70867728950e.webp"));
        for bad in [
            "",
            "avatar.png",
            "0f8fad5b-d9cb-469f-a165-70867728950e",
            "0f8fad5b-d9cb-469f-a165-70867728950e.gif",
            "0F8FAD5B-D9CB-469F-A165-70867728950E.png",
            "0f8fad5bd9cb469fa16570867728950e.png",
            "../0f8fad5b-d9cb-469f-a165-70867728950e.png",
            "urn:uuid:0f8fad5b-d9cb-469f-a165-70867728950e.png",
            "0f8fad5b-d9cb-469f-a165-70867728950e.png/../../x",
            "C:\\0f8fad5b-d9cb-469f-a165-70867728950e.png",
        ] {
            assert!(!is_valid_name(bad), "{bad:?} must not be taken as an avatar name");
        }
    }

    #[test]
    fn saves_and_reads_back_under_a_minted_name() {
        let dir = TempDir::new();
        let name = save(&dir.0, WEBP).expect("save");
        assert!(is_valid_name(&name));
        assert!(name.ends_with(".webp"));
        assert_eq!(read(&dir.0, &name), Some((WEBP.to_vec(), Format::Webp)));

        let again = save(&dir.0, WEBP).expect("save again");
        assert_ne!(name, again, "a new picture always gets a new name");
    }

    #[test]
    fn refuses_what_isnt_an_image() {
        let dir = TempDir::new();
        assert!(save(&dir.0, b"<svg onload=alert(1)>").is_err());
        assert!(save(&dir.0, &vec![0xFF; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn sweep_keeps_what_is_referenced_and_ignores_foreign_files() {
        let dir = TempDir::new();
        let kept = save(&dir.0, PNG).expect("save");
        let orphan = save(&dir.0, PNG).expect("save");
        std::fs::write(dir.0.join("notes.txt"), "mine").expect("write");

        let removed = sweep(&dir.0, &HashSet::from([kept.clone()]));
        assert_eq!(removed, 1);
        assert!(exists(&dir.0, &kept));
        assert!(!exists(&dir.0, &orphan));
        assert!(dir.0.join("notes.txt").exists());
    }

    #[test]
    fn decodes_data_urls() {
        let url = to_data_url(PNG, Format::Png);
        assert_eq!(decode_data_url(&url).expect("decode"), PNG);
        assert!(decode_data_url("data:image/png,rawtext").is_err());
        assert!(decode_data_url("https://example.com/a.png").is_err());
    }

    #[test]
    fn serves_stored_pictures_and_nothing_else() {
        let dir = TempDir::new();
        let name = save(&dir.0, PNG).expect("save");

        let ok = serve(Some(&dir.0), &format!("/{name}"));
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(ok.body(), PNG);

        assert_eq!(
            serve(Some(&dir.0), "/../voicechat.db").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(serve(None, &format!("/{name}")).status(), StatusCode::NOT_FOUND);
    }
}
