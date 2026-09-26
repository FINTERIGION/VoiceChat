//! One character as a file for handing to someone else: who they are, what
//! they look like if the sender chose to include that, and what they sound
//! like. Nothing else.
//!
//! A backup (`store::backup`) carries one person's whole setup to another of
//! their own devices. This is different in every way that matters for giving
//! a character away:
//!
//! - Memories and conversations stay behind. They are about the person who
//!   talked to the character, not about the character.
//! - There are no ids. Importing always creates a new character, so the same
//!   file imported twice gives you two.
//! - No settings, and never the API key.
//! - The voice travels as what it was made *from* rather than as a voice id.
//!   A cloned or designed voice lives in the sender's DashScope account, and
//!   its id means nothing in anyone else's, so the file carries a description
//!   or an audio sample instead, and importing makes the voice again under
//!   the recipient's own account (see `app::commands::import_character`).
//!   Preset voices are the exception: those are the same names for everyone.
//!
//! Pictures and audio are `data:` URLs, so the file is still one JSON
//! document a person can read, or write by hand. A name and a sentence
//! describing a voice is already a complete character.

use serde::{Deserialize, Serialize};

use crate::avatar;
use crate::store::{backup, character};
use crate::voice::{sample, service::PRESET_VOICES};

/// Stamped into every file and checked on the way back in, so picking the
/// wrong JSON file fails with "this isn't a character" rather than a
/// complaint about some missing field.
pub const FORMAT: &str = "voice-chat-character";

/// Bumped only when the shape changes such that an older build would misread
/// a newer file. Import accepts anything up to this.
pub const VERSION: u32 = 1;

/// Ceilings checked before anything in a file is acted on. This file arrives
/// from someone else by design, and what it holds ends up in the model
/// instructions (`persona`, `speech_habits`) and in requests to the voice
/// API, so it gets the same scrutiny a backup does.
mod limits {
    /// The same as a backup's, for the same reason: both prose fields go
    /// into the instructions on every connect.
    pub const PROSE_CHARS: usize = 20_000;
    /// What Voice Design accepts, and what the Voice Studio's box allows.
    pub const VOICE_PROMPT_CHARS: usize = 500;
    pub const PREVIEW_TEXT_CHARS: usize = 1_000;
    pub const MAX_HISTORY_TURNS: i64 = 200;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedCharacter {
    pub format: String,
    pub version: u32,
    pub name: String,
    /// One of `backup::LANGUAGES`. A hand-written file can leave it out and
    /// get a character that answers in whatever language it is spoken to in.
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub persona: String,
    #[serde(default)]
    pub speech_habits: String,
    /// A `data:` URL. Absent when the sender left the picture out, or the
    /// character never had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub voice: SharedVoice,
    /// The two remaining things the editor lets you set. Optional, so a
    /// hand-written file gets the defaults a new character gets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_history_turns: Option<i64>,
}

fn default_language() -> String {
    "auto".into()
}

/// What the recipient's copy of the voice is made from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SharedVoice {
    /// One of `PRESET_VOICES`, used as it is.
    Preset { id: String },
    /// A Voice Design description. The voice made from it sounds like the
    /// original rather than identical to it: design is not deterministic.
    Description {
        prompt: String,
        /// What the designed voice reads aloud for the sample the realtime
        /// voice is cloned from. Absent means `default_preview_text`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview_text: Option<String>,
    },
    /// A recording to clone, as a `data:` URL.
    Audio { data: String },
}

/// `Err` carries a message naming the offending field, since the only useful
/// thing a user can do with a rejected file is look at it, or tell whoever
/// sent it.
///
/// The checks below give only the reason; `SharedCharacter::normalize` says
/// it is the file that isn't usable. An export checks the voice on its own
/// first, and there it is a sample the user just picked, not a file.
type Check = Result<(), String>;

fn reject(what: impl std::fmt::Display) -> String {
    crate::tr!(
        format!("This character file isn't usable: {what}"),
        format!("该角色文件无法使用：{what}"),
    )
}

fn text(value: &str, max: usize, field: &str) -> Check {
    if value.chars().count() > max {
        return Err(crate::tr!(
            format!("{field} is longer than the {max} characters allowed"),
            format!("{field} 超过了允许的 {max} 个字符"),
        ));
    }
    Ok(())
}

/// The longest `data:` URL that could hold `max_bytes`, header included.
/// Checked before decoding, so an oversized file is turned away without
/// first being copied into memory a second time.
fn data_url_chars(max_bytes: usize) -> usize {
    (max_bytes / 3 + 1) * 4 + 64
}

/// The picture relabelled with the type its bytes are.
fn check_avatar(url: &str) -> Result<String, String> {
    let too_large = || {
        crate::tr!(
            format!(
                "the avatar is larger than {} MB",
                avatar::MAX_BYTES / 1_048_576
            ),
            format!("头像超过了 {} MB", avatar::MAX_BYTES / 1_048_576),
        )
    };
    if url.len() > data_url_chars(avatar::MAX_BYTES) {
        return Err(too_large());
    }
    let (bytes, format) = avatar::decode_data_url(url)
        .ok()
        .and_then(|bytes| avatar::Format::sniff(&bytes).map(|format| (bytes, format)))
        .ok_or_else(|| {
            crate::tr!(
                "the avatar isn't a PNG, JPEG or WebP image",
                "头像不是 PNG、JPEG 或 WebP 图片",
            )
            .to_string()
        })?;
    if bytes.len() > avatar::MAX_BYTES {
        return Err(too_large());
    }
    Ok(avatar::to_data_url(&bytes, format))
}

/// The recording relabelled with the type its bytes are — which is what the
/// webview's `<audio>` and the cloning API go by.
fn check_audio(url: &str) -> Result<String, String> {
    let too_large = || {
        crate::tr!(
            format!(
                "the voice sample is larger than {} MB",
                sample::MAX_BYTES / 1_048_576
            ),
            format!("音频样本超过了 {} MB", sample::MAX_BYTES / 1_048_576),
        )
    };
    if url.len() > data_url_chars(sample::MAX_BYTES) {
        return Err(too_large());
    }
    let (bytes, format) = avatar::decode_data_url(url)
        .ok()
        .and_then(|bytes| sample::Format::sniff(&bytes).map(|format| (bytes, format)))
        .ok_or_else(|| {
            crate::tr!(
                format!("the voice sample isn't a {} recording", sample::names()),
                format!("音频样本不是 {} 格式", sample::names()),
            )
        })?;
    if bytes.len() > sample::MAX_BYTES {
        return Err(too_large());
    }
    Ok(sample::to_data_url(&bytes, format))
}

impl SharedVoice {
    /// The voice checked and tidied as `SharedCharacter::normalize` describes.
    /// `Err` is only the reason, without saying what it is the reason for.
    pub fn normalize(self) -> Result<Self, String> {
        match self {
            Self::Preset { id } => {
                if !PRESET_VOICES.contains(&id.as_str()) {
                    return Err(crate::tr!(
                        format!("{id:?} isn't a preset voice this version knows"),
                        format!("{id:?} 不是本版本已知的预置音色"),
                    ));
                }
                Ok(Self::Preset { id })
            }
            Self::Description {
                prompt,
                preview_text,
            } => {
                let prompt = prompt.trim().to_string();
                if prompt.is_empty() {
                    return Err(crate::tr!(
                        "the voice description is empty",
                        "声音描述为空",
                    )
                    .to_string());
                }
                text(&prompt, limits::VOICE_PROMPT_CHARS, "voice.prompt")?;
                let preview_text = preview_text
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                if let Some(t) = &preview_text {
                    text(t, limits::PREVIEW_TEXT_CHARS, "voice.preview_text")?;
                }
                Ok(Self::Description {
                    prompt,
                    preview_text,
                })
            }
            Self::Audio { data } => Ok(Self::Audio {
                data: check_audio(&data)?,
            }),
        }
    }
}

impl SharedCharacter {
    /// What goes into a file for `c`. `avatar` is the picture as a `data:`
    /// URL, if it is to be included; `voice` is chosen by the user, since
    /// only they can supply a sample for a voice this device never kept one
    /// of.
    pub fn from_character(
        c: &character::Character,
        avatar: Option<String>,
        voice: SharedVoice,
    ) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            name: c.name.clone(),
            language: c.language.clone(),
            persona: c.persona.clone(),
            speech_habits: c.speech_habits.clone(),
            avatar,
            voice,
            memory_enabled: Some(c.memory_enabled),
            max_history_turns: Some(c.max_history_turns),
        }
    }

    /// Reads a file's text, telling a backup apart from a file that is
    /// neither — the one mix-up worth a pointer to where it does belong.
    pub fn parse(raw: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Header {
            #[serde(default)]
            format: String,
        }
        let header: Header = serde_json::from_str(raw).map_err(|e| {
            crate::tr!(
                format!("This file isn't a readable Voice Chat character: {e}"),
                format!("无法读取该 Voice Chat 角色文件：{e}"),
            )
        })?;
        if header.format == backup::FORMAT {
            return Err(crate::tr!(
                "This is a backup, not a shared character — restore it from Settings › Backup & restore instead",
                "这是备份文件，不是分享的角色。请到「设置 › 备份与恢复」中恢复它",
            )
            .into());
        }
        if header.format != FORMAT {
            return Err(crate::tr!(
                "This file isn't a Voice Chat character",
                "这不是 Voice Chat 的角色文件",
            )
            .into());
        }
        serde_json::from_str(raw).map_err(|e| {
            crate::tr!(
                format!("This file isn't a readable Voice Chat character: {e}"),
                format!("无法读取该 Voice Chat 角色文件：{e}"),
            )
        })
    }

    /// Checks everything, and returns the character as it should be used:
    /// the name and voice description trimmed, and each `data:` URL
    /// relabelled with the type its bytes actually are.
    ///
    /// Strict rather than forgiving, like a backup's `validate`: quietly
    /// clamping a file would import something other than what it says.
    pub fn normalize(self) -> Result<Self, String> {
        if self.format != FORMAT {
            return Err(crate::tr!(
                "This file isn't a Voice Chat character",
                "这不是 Voice Chat 的角色文件",
            )
            .into());
        }
        if self.version > VERSION {
            return Err(crate::tr!(
                format!(
                    "This character was shared from a newer version of Voice Chat (format v{}) — update the app first",
                    self.version
                ),
                format!(
                    "该角色由更新版本的 Voice Chat 导出（格式 v{}），请先升级应用",
                    self.version
                ),
            ));
        }

        let name = character::check_name(&self.name).map_err(reject)?;
        if !backup::LANGUAGES.contains(&self.language.as_str()) {
            return Err(reject(crate::tr!(
                format!(
                    "language is {:?}, which is not one of {:?}",
                    self.language,
                    backup::LANGUAGES
                ),
                format!(
                    "language 的值 {:?} 不在允许的取值 {:?} 中",
                    self.language,
                    backup::LANGUAGES
                ),
            )));
        }
        text(&self.persona, limits::PROSE_CHARS, "persona").map_err(reject)?;
        text(&self.speech_habits, limits::PROSE_CHARS, "speech_habits").map_err(reject)?;
        // Sent to the API as the session's history window, so an absurd
        // value is a request this app would never make on its own.
        if let Some(turns) = self.max_history_turns {
            if !(1..=limits::MAX_HISTORY_TURNS).contains(&turns) {
                return Err(reject(crate::tr!(
                    format!(
                        "max_history_turns is {turns}, outside the range 1-{}",
                        limits::MAX_HISTORY_TURNS
                    ),
                    format!(
                        "max_history_turns 的值 {turns} 超出了 1-{} 的范围",
                        limits::MAX_HISTORY_TURNS
                    ),
                )));
            }
        }
        let avatar = self
            .avatar
            .as_deref()
            .map(check_avatar)
            .transpose()
            .map_err(reject)?;
        let voice = self.voice.normalize().map_err(reject)?;

        Ok(Self {
            name,
            avatar,
            voice,
            ..self
        })
    }

    /// The picture's bytes, ready for `avatar::save`. Only vouched for once
    /// `normalize` has passed.
    pub fn avatar_bytes(&self) -> Option<Vec<u8>> {
        self.avatar
            .as_deref()
            .and_then(|url| avatar::decode_data_url(url).ok())
    }
}

/// What the designed voice reads for its sample when a file doesn't say:
/// 150-odd characters, the length the Voice Studio asks for, which is enough
/// speech for the realtime voice to be cloned from.
///
/// Only two languages, because the voice description itself can only be
/// Chinese or English. What cloning takes from the sample is the timbre; the
/// realtime model then speaks whatever language the persona calls for in it,
/// so a Japanese character gets the Chinese passage. A character that
/// follows the user's language goes by the script its description is written
/// in.
pub fn default_preview_text(language: &str, prompt: &str) -> &'static str {
    const ZH: &str = "你好呀，很高兴认识你。今天过得怎么样？不管是开心的事，还是有点烦心的事，都可以慢慢说给我听，我会认真听完，再陪你一起想想办法。天气好的时候，我喜欢出去走一走，看看路边的花草树木；安静的晚上，就泡一杯热茶，读几页喜欢的书。你呢？平时最喜欢做些什么？不用着急，我们有的是时间，想到哪里就聊到哪里，说不定会聊出很多有意思的话题呢。";
    const EN: &str = "Hi there, it's really nice to meet you. How has your day been so far? Whether something good happened or something has been bothering you, take your time and tell me all about it. I'll listen carefully, and then we can think it through together. When the weather is nice, I love going for a walk and looking at the trees and flowers along the way, and on quiet evenings I like to make a cup of tea and read a few pages of a good book. What about you? What do you enjoy doing most? There's no rush at all, so let's just talk about whatever comes to mind.";
    let is_cjk = |c: char| matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF);
    match language {
        "en" => EN,
        "zh" | "ja" => ZH,
        _ if prompt.chars().any(is_cjk) => ZH,
        _ => EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0";

    /// A file whose `data:` URLs declare the wrong type, as a hand-made one
    /// easily would.
    fn shared(voice: SharedVoice) -> SharedCharacter {
        SharedCharacter {
            format: FORMAT.into(),
            version: VERSION,
            name: "  Nia ".into(),
            language: "en".into(),
            persona: "curious and warm".into(),
            speech_habits: "short sentences".into(),
            avatar: Some(format!(
                "data:application/octet-stream;base64,{}",
                avatar::encode(PNG)
            )),
            voice,
            memory_enabled: Some(false),
            max_history_turns: Some(30),
        }
    }

    fn audio() -> SharedVoice {
        SharedVoice::Audio {
            data: format!("data:audio/x-wav;base64,{}", avatar::encode(WAV)),
        }
    }

    #[test]
    fn round_trips_through_json() {
        let json = serde_json::to_string(&shared(audio())).expect("serialize");
        let parsed = SharedCharacter::parse(&json)
            .expect("parse")
            .normalize()
            .expect("valid");

        assert_eq!(parsed.name, "Nia", "the name is trimmed");
        assert_eq!(parsed.memory_enabled, Some(false));
        assert_eq!(parsed.max_history_turns, Some(30));
        assert_eq!(parsed.avatar_bytes().as_deref(), Some(PNG));
        assert!(
            parsed.avatar.as_deref().unwrap().starts_with("data:image/png;base64,"),
            "the picture is relabelled with what it is"
        );
        match parsed.voice {
            SharedVoice::Audio { data } => {
                assert!(data.starts_with("data:audio/wav;base64,"));
            }
            other => panic!("the voice changed kind: {other:?}"),
        }
    }

    /// The whole point of keeping the format plain: a name and a sentence
    /// about a voice is a character.
    #[test]
    fn a_minimal_hand_written_file_imports() {
        let raw = r#"{
            "format": "voice-chat-character",
            "version": 1,
            "name": "小柔",
            "voice": { "kind": "description", "prompt": "  温柔的女声  " }
        }"#;
        let parsed = SharedCharacter::parse(raw)
            .expect("parse")
            .normalize()
            .expect("valid");
        assert_eq!(parsed.language, "auto");
        assert_eq!(parsed.persona, "");
        assert_eq!(parsed.avatar, None);
        assert_eq!(parsed.memory_enabled, None);
        match parsed.voice {
            SharedVoice::Description {
                prompt,
                preview_text,
            } => {
                assert_eq!(prompt, "温柔的女声");
                assert_eq!(preview_text, None);
            }
            other => panic!("the voice changed kind: {other:?}"),
        }
    }

    #[test]
    fn points_a_backup_at_where_it_belongs() {
        let backup = r#"{"format": "voice-chat-backup", "version": 2}"#;
        let err = SharedCharacter::parse(backup).expect_err("not a character");
        assert!(err.contains("Backup") || err.contains("备份"), "{err}");

        assert!(SharedCharacter::parse(r#"{"hello": "world"}"#).is_err());
        assert!(SharedCharacter::parse("not json").is_err());
    }

    #[test]
    fn rejects_files_it_cannot_import() {
        let mut newer = shared(audio());
        newer.version = VERSION + 1;
        assert!(newer.normalize().is_err(), "a newer format");

        let mut long = shared(audio());
        long.name = "字".repeat(13);
        assert!(long.normalize().is_err(), "a name the editor wouldn't take");

        let mut language = shared(audio());
        language.language = "de".into();
        assert!(language.normalize().is_err(), "a language outside the set");

        let mut persona = shared(audio());
        persona.persona = "x".repeat(limits::PROSE_CHARS + 1);
        assert!(persona.normalize().is_err(), "an oversized persona");

        for turns in [0, limits::MAX_HISTORY_TURNS + 1] {
            let mut c = shared(audio());
            c.max_history_turns = Some(turns);
            assert!(c.normalize().is_err(), "max_history_turns {turns}");
        }
    }

    #[test]
    fn rejects_voices_it_cannot_make() {
        let preset = |id: &str| shared(SharedVoice::Preset { id: id.into() });
        assert!(preset("longanqian").normalize().is_ok());
        assert!(preset("someone-elses-cloned-voice").normalize().is_err());

        let description = |prompt: String| {
            shared(SharedVoice::Description {
                prompt,
                preview_text: None,
            })
        };
        assert!(description("   ".into()).normalize().is_err(), "blank");
        assert!(
            description("x".repeat(limits::VOICE_PROMPT_CHARS + 1))
                .normalize()
                .is_err(),
            "longer than Voice Design takes"
        );

        let audio = |data: String| shared(SharedVoice::Audio { data });
        assert!(
            audio(format!("data:audio/wav;base64,{}", avatar::encode(PNG)))
                .normalize()
                .is_err(),
            "a picture labelled as audio"
        );
        assert!(audio("data:audio/wav,raw".into()).normalize().is_err());
        assert!(audio("https://example.com/a.wav".into()).normalize().is_err());
    }

    /// Voice enrollment clones from OGG though its documentation doesn't say
    /// so, and game voice lines are often OGG saved under a `.wav` name.
    #[test]
    fn takes_an_ogg_sample_whatever_it_is_labelled() {
        let ogg = SharedVoice::Audio {
            data: format!(
                "data:audio/wav;base64,{}",
                avatar::encode(b"OggS\0\x02\0\0\0\0\0\0\0\0")
            ),
        };
        match shared(ogg).normalize().expect("valid").voice {
            SharedVoice::Audio { data } => {
                assert!(data.starts_with("data:audio/ogg;base64,"), "{data}");
            }
            other => panic!("the voice changed kind: {other:?}"),
        }
    }

    /// An export checks the voice by itself, where "this file isn't usable"
    /// would be wrong: there is no file yet.
    #[test]
    fn a_voice_on_its_own_is_rejected_without_blaming_a_file() {
        let blank = || SharedVoice::Description {
            prompt: " ".into(),
            preview_text: None,
        };
        let alone = blank().normalize().expect_err("blank");
        let in_file = shared(blank()).normalize().expect_err("blank");
        assert!(in_file.ends_with(&alone), "{in_file}");
        assert_ne!(in_file, alone);
    }

    #[test]
    fn rejects_an_avatar_that_isnt_an_image() {
        let mut c = shared(audio());
        c.avatar = Some(format!(
            "data:image/png;base64,{}",
            avatar::encode(b"<svg onload=alert(1)>")
        ));
        assert!(c.normalize().is_err());
    }

    #[test]
    fn picks_a_sample_passage_by_language() {
        let zh = default_preview_text("zh", "");
        let en = default_preview_text("en", "");
        assert_ne!(zh, en);
        assert_eq!(default_preview_text("ja", ""), zh);
        assert_eq!(default_preview_text("auto", "温柔的女声"), zh);
        assert_eq!(default_preview_text("auto", "a warm voice"), en);
        // The length the Voice Studio asks for.
        assert!(zh.chars().count() >= 150, "{}", zh.chars().count());
        assert!(en.chars().count() >= 150);
    }
}
