//! The audio a custom voice was cloned from.
//!
//! Kept as files in the app data directory's `voice_samples/` folder, one per
//! voice (`store::voice_sample` records which), so that sharing a character
//! can hand over what its voice was made from rather than a voice id that
//! means nothing in anyone else's account (see `store::share`). A sample
//! arrives with every clone the app makes — a recording, a picked file, or
//! the preview a designed voice is cloned from — and with a shared file's.
//!
//! Named and guarded the same way as avatars: every name is a fresh UUID
//! plus the extension of the format the bytes really are, minted here, so a
//! stored name that isn't shaped like one reaches nothing.

use std::collections::HashSet;
use std::path::Path;

pub const DIR_NAME: &str = "voice_samples";

/// Comfortably above the 10–20 seconds of speech voice enrollment asks for,
/// and above the in-app recorder's longest take, which is a 24 kHz mono WAV
/// (under 3 MB for its 60 seconds; see `voice::clone`).
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

/// The formats voice enrollment clones from. Its documentation lists only
/// WAV, MP3 and M4A; these are what it actually took when each was tried
/// against it, whatever the codec or bit depth inside. AIFF, WMA and AMR it
/// turns away. It goes by the bytes rather than the declared type, as
/// `sniff` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Mp3,
    M4a,
    Flac,
    Ogg,
    Aac,
    Webm,
}

/// The formats as messages list them.
pub fn names() -> &'static str {
    crate::tr!(
        "WAV, MP3, M4A, FLAC, OGG, AAC or WebM",
        "WAV、MP3、M4A、FLAC、OGG、AAC 或 WebM",
    )
}

impl Format {
    /// Decided by the bytes, never by the type a `data:` URL declares.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        // An ID3 tag says nothing about what follows it. Usually that's MP3,
        // but FLAC and AAC files carry them too.
        if let Some(rest) = after_id3(bytes) {
            return Some(match Self::sniff_untagged(rest) {
                Some(format @ (Self::Flac | Self::Aac)) => format,
                _ => Self::Mp3,
            });
        }
        Self::sniff_untagged(bytes)
    }

    fn sniff_untagged(bytes: &[u8]) -> Option<Self> {
        // MP3 frames and AAC's ADTS frames share their sync bits; the layer
        // bits tell them apart, zero for AAC.
        let frame_sync = bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0;
        if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
            Some(Self::Wav)
        } else if frame_sync && bytes[1] & 0x06 != 0 {
            Some(Self::Mp3)
        } else if frame_sync && bytes[1] & 0xF6 == 0xF0 {
            Some(Self::Aac)
        } else if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
            Some(Self::M4a)
        } else if bytes.starts_with(b"fLaC") {
            Some(Self::Flac)
        } else if bytes.starts_with(b"OggS") {
            Some(Self::Ogg)
        } else if is_webm(bytes) {
            Some(Self::Webm)
        } else {
            None
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Aac => "aac",
            Self::Webm => "webm",
        }
    }

    fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "wav" => Some(Self::Wav),
            "mp3" => Some(Self::Mp3),
            "m4a" => Some(Self::M4a),
            "flac" => Some(Self::Flac),
            "ogg" => Some(Self::Ogg),
            "aac" => Some(Self::Aac),
            "webm" => Some(Self::Webm),
            _ => None,
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::M4a => "audio/mp4",
            Self::Flac => "audio/flac",
            Self::Ogg => "audio/ogg",
            Self::Aac => "audio/aac",
            Self::Webm => "audio/webm",
        }
    }
}

/// What follows an ID3v2 tag at the start of `bytes`, if one is there —
/// empty when the tag runs to the end.
fn after_id3(bytes: &[u8]) -> Option<&[u8]> {
    if !bytes.starts_with(b"ID3") {
        return None;
    }
    let Some(header) = bytes.get(..10) else {
        return Some(&[]);
    };
    // Four 7-bit bytes, then a footer the same size as the header if the
    // flags say there is one.
    let size = header[6..10]
        .iter()
        .fold(0usize, |size, &b| (size << 7) | usize::from(b & 0x7F));
    let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };
    Some(bytes.get(10 + size + footer..).unwrap_or(&[]))
}

/// An EBML header naming WebM as its document type. Matroska proper
/// (`.mka`, `.mkv`) starts the same way but wasn't tried against the API, so
/// it isn't taken for WebM.
fn is_webm(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
        && bytes[4..bytes.len().min(64)]
            .windows(4)
            .any(|w| w == b"webm")
}

pub fn to_data_url(bytes: &[u8], format: Format) -> String {
    format!("data:{};base64,{}", format.mime(), crate::avatar::encode(bytes))
}

/// The bytes of a `data:…;base64,…` URL, if they are a sample this module
/// can keep. `None` for anything else — a public URL, an unsupported format,
/// something over `MAX_BYTES`.
pub fn from_data_url(url: &str) -> Option<Vec<u8>> {
    let bytes = crate::avatar::decode_data_url(url).ok()?;
    (bytes.len() <= MAX_BYTES && Format::sniff(&bytes).is_some()).then_some(bytes)
}

/// Whether `name` is one `save` could have minted.
pub fn is_valid_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    crate::avatar::is_minted_stem(stem) && Format::from_ext(ext).is_some()
}

/// Stores a sample and returns the name to record against its voice.
pub fn save(dir: &Path, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err(crate::tr!(
            format!(
                "That recording is too large to keep (the limit is {} MB)",
                MAX_BYTES / 1_048_576
            ),
            format!("音频太大，无法保存（上限 {} MB）", MAX_BYTES / 1_048_576),
        ));
    }
    let format = Format::sniff(bytes).ok_or_else(|| {
        crate::tr!(
            format!("Only {} recordings can be kept", names()),
            format!("只能保存 {} 格式的音频", names()),
        )
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

/// Deletes a sample nothing points at any more. Failing to is not worth
/// failing whatever let go of it over: `sweep` gets another go at the next
/// launch.
pub fn remove(dir: &Path, name: &str) {
    if !is_valid_name(name) {
        return;
    }
    match std::fs::remove_file(dir.join(name)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("failed to remove voice sample {name}: {e}"),
    }
}

/// Removes every sample in `dir` that isn't in `keep`, touching only files
/// named the way `save` names them. Runs at startup, like `avatar::sweep`.
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

#[cfg(test)]
mod tests {
    use super::*;

    const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt ";

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("voicechat-voice-sample-{}", uuid::Uuid::new_v4()));
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
        assert_eq!(Format::sniff(WAV), Some(Format::Wav));
        assert_eq!(Format::sniff(b"ID3\x04\0\0\0\0\0\0"), Some(Format::Mp3));
        assert_eq!(Format::sniff(&[0xFF, 0xFB, 0x90, 0x64]), Some(Format::Mp3));
        assert_eq!(Format::sniff(b"\0\0\0\x20ftypM4A "), Some(Format::M4a));
        assert_eq!(Format::sniff(b"fLaC\0\0\0\x22"), Some(Format::Flac));
        assert_eq!(Format::sniff(b"OggS\0\x02\0\0"), Some(Format::Ogg));
        // An `.opus` file is Opus in an Ogg container, so it is OGG here.
        let opus = b"OggS\0\x02\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01\x13OpusHead";
        assert_eq!(Format::sniff(opus), Some(Format::Ogg));
        assert_eq!(Format::sniff(&[0xFF, 0xF1, 0x50, 0x80]), Some(Format::Aac));
        let webm = b"\x1A\x45\xDF\xA3\x9F\x42\x86\x81\x01\x42\x82\x84webm\x42\x87\x81\x04";
        assert_eq!(Format::sniff(webm), Some(Format::Webm));

        // Tagged files are what the tag is in front of.
        assert_eq!(Format::sniff(b"ID3\x04\0\0\0\0\0\x02\0\0fLaC"), Some(Format::Flac));
        assert_eq!(
            Format::sniff(b"ID3\x04\0\0\0\0\0\0\xFF\xF1\x50\x80"),
            Some(Format::Aac)
        );
        assert_eq!(Format::sniff(b"ID3\x04\0\0\0\0\0\0\xFF\xFB\x90\x64"), Some(Format::Mp3));

        assert_eq!(Format::sniff(b"RIFF\x24\0\0\0WEBPVP8 "), None, "a WebP image");
        assert_eq!(Format::sniff(b"FORM\0\0\0\x24AIFFCOMM"), None, "AIFF, which cloning refuses");
        let matroska = b"\x1A\x45\xDF\xA3\xA3\x42\x86\x81\x01\x42\x82\x88matroska";
        assert_eq!(Format::sniff(matroska), None, "Matroska that isn't WebM");
        assert_eq!(Format::sniff(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(Format::sniff(b""), None);
    }

    #[test]
    fn keeps_only_what_can_be_cloned_from() {
        assert_eq!(
            from_data_url(&to_data_url(WAV, Format::Wav)).as_deref(),
            Some(WAV)
        );
        assert_eq!(from_data_url("https://example.com/voice.wav"), None);
        assert_eq!(
            from_data_url(&format!(
                "data:audio/aiff;base64,{}",
                crate::avatar::encode(b"FORM\0\0\0\x24AIFFCOMM")
            )),
            None,
            "a format voice enrollment turns away"
        );
    }

    #[test]
    fn saves_reads_and_sweeps_under_minted_names() {
        let dir = TempDir::new();
        let kept = save(&dir.0, WAV).expect("save");
        assert!(is_valid_name(&kept));
        assert!(kept.ends_with(".wav"));
        assert_eq!(read(&dir.0, &kept), Some((WAV.to_vec(), Format::Wav)));

        let orphan = save(&dir.0, WAV).expect("save");
        std::fs::write(dir.0.join("notes.txt"), "mine").expect("write");
        assert_eq!(sweep(&dir.0, &HashSet::from([kept.clone()])), 1);
        assert!(read(&dir.0, &kept).is_some());
        assert!(read(&dir.0, &orphan).is_none());
        assert!(dir.0.join("notes.txt").exists());

        assert!(save(&dir.0, b"<svg onload=alert(1)>").is_err());
        for bad in ["../voicechat.db", "sample.wav", "0f8fad5b-d9cb-469f-a165-70867728950e.png"] {
            assert!(!is_valid_name(bad), "{bad:?}");
            assert!(read(&dir.0, bad).is_none());
        }
    }
}
