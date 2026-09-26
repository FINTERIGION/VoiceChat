//! One file holding everything the user has accumulated: their characters,
//! each character's long-term memories, every conversation they have had with
//! them, and the settings that point the app at their account.
//!
//! The point is moving to another machine, so the file is self-contained
//! JSON rather than a copy of the SQLite database: it survives a schema
//! change (an older backup restored into a newer build), can be inspected
//! and hand-edited, and carries the API key — which lives in the OS keyring
//! rather than in the database, and so has to be read and written separately
//! at both ends.
//!
//! Carrying the key is what makes a restore seamless instead of a to-do
//! list, and it is also what makes the file a secret: whoever holds it can
//! spend the user's DashScope account. So the export says as much before it
//! writes, and `include_api_key` lets someone who is sharing characters
//! rather than moving devices leave the key behind.
//!
//! Cloned and designed voices aren't in here: those live in the DashScope
//! account, so a character keeps its own voice on the new machine as long as
//! the same key is configured there — which, now, the backup does itself.
//! The audio each one was cloned from is (`voice_samples`), since that is
//! kept on this device (see `crate::voice::sample`) and is what sharing a
//! character hands over.
//!
//! Avatars are, as base64 inside each character (`avatar_data`): they are
//! files on this device (see `crate::avatar`), and a file name alone would
//! point at nothing on the next one.

use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::avatar;
use crate::store::{character, db, memory, voice_sample};
use crate::voice::sample;

/// Stamped into every file and checked on the way back in, so picking the
/// wrong JSON file fails with "this isn't a backup" instead of importing
/// nothing and reporting success.
pub const FORMAT: &str = "voicechat-backup";

/// Bumped only when the shape changes such that an older build would misread
/// a newer file. Import accepts anything up to this.
///
/// v2 added `settings`. A v1 file still restores — the block is optional, and
/// its absence means "this device keeps the settings it has" — but a v2 file
/// must not land in a v1 build, which would drop the settings silently and
/// report a complete restore.
pub const VERSION: u32 = 2;

/// Ceilings on what a file may contain, checked before any of it reaches the
/// database.
///
/// A backup is the one input to this app that arrives as a whole file from
/// somewhere else — the feature exists to carry it between machines — so it is
/// the one place where "the user typed this" stops being true. Anything this
/// app exported sits far under every limit here; a file that doesn't is
/// either damaged or built to be, and neither is worth restoring.
///
/// The counts bound how much work one import can queue up, and the lengths
/// bound the individual strings — which matter beyond storage, because
/// `persona`, `speech_habits` and memory `content` are concatenated into the
/// model instructions on every connect (see `prompt::builder`).
mod limits {
    pub const CHARACTERS: usize = 500;
    pub const MEMORIES_PER_CHARACTER: usize = 1_000;
    pub const CONVERSATIONS_PER_CHARACTER: usize = 10_000;
    pub const MESSAGES_PER_CONVERSATION: usize = 10_000;

    pub const ID_CHARS: usize = 128;
    pub const TIMESTAMP_CHARS: usize = 64;
    pub const NAME_CHARS: usize = 200;
    pub const TITLE_CHARS: usize = 200;
    pub const PATH_CHARS: usize = 4_096;
    /// An avatar at the largest size `avatar::save` accepts, base64-encoded.
    pub const AVATAR_DATA_CHARS: usize = (crate::avatar::MAX_BYTES / 3 + 1) * 4;
    /// Likewise a voice sample at the largest size `voice::sample::save`
    /// accepts.
    pub const SAMPLE_DATA_CHARS: usize = (crate::voice::sample::MAX_BYTES / 3 + 1) * 4;
    pub const VOICE_ID_CHARS: usize = 200;
    /// Covers `persona`, `speech_habits`, `voice_prompt` and memory content.
    pub const PROSE_CHARS: usize = 20_000;
    pub const MESSAGE_CHARS: usize = 100_000;

    pub const MAX_HISTORY_TURNS: i64 = 200;

    /// Model Studio keys are ~35 characters; the ceiling is only here so a
    /// file can't hand the OS keyring a megabyte.
    pub const API_KEY_CHARS: usize = 512;
    pub const HOTKEY_CHARS: usize = 128;

    /// Wider than the slider in Settings, which writes 200-6000. The range
    /// only has to exclude values that would make a session's turn detection
    /// nonsense, not second-guess a hand-edited file that stays sane.
    pub const MIN_VAD_SILENCE_MS: i64 = 100;
    pub const MAX_VAD_SILENCE_MS: i64 = 60_000;
}

/// The closed sets the schema comments describe. Stored as-is today, but
/// `language` and `voice_kind` steer the prompt and the realtime `voice`,
/// and `role` decides who a transcript line is attributed to when a
/// conversation is summarized — so a value outside the set is a file that
/// would drive this app somewhere its own UI cannot.
pub(crate) const LANGUAGES: &[&str] = &["zh", "ja", "en", "auto"];
const VOICE_KINDS: &[&str] = &["preset", "designed", "cloned"];
const MEMORY_KINDS: &[&str] = &["profile", "fact", "summary", "open_loop"];
const MESSAGE_ROLES: &[&str] = &["user", "assistant"];

/// The tags `i18n::Lang::from_tag` recognises. It falls back to English for
/// anything else rather than erroring — a settings row must never be able to
/// stop the app starting — so an unknown tag would import happily and then
/// not be the language the file asked for.
const UI_LANGUAGE_TAGS: &[&str] = &["en", "zh-CN"];

/// `Err` carries a message naming the offending field, since the only useful
/// thing a user can do with a rejected file is look at it.
type Check = Result<(), String>;

fn reject(what: String) -> String {
    crate::tr!(
        format!("This backup file isn't usable: {what}"),
        format!("该备份文件无法使用：{what}"),
    )
}

fn text(value: &str, max: usize, field: &str) -> Check {
    if value.chars().count() > max {
        return Err(reject(crate::tr!(
            format!("{field} is longer than the {max} characters allowed"),
            format!("{field} 超过了允许的 {max} 个字符"),
        )));
    }
    Ok(())
}

fn required(value: &str, max: usize, field: &str) -> Check {
    if value.trim().is_empty() {
        return Err(reject(crate::tr!(
            format!("{field} is empty"),
            format!("{field} 为空"),
        )));
    }
    text(value, max, field)
}

fn one_of(value: &str, allowed: &[&str], field: &str) -> Check {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(reject(crate::tr!(
        format!("{field} is {value:?}, which is not one of {allowed:?}"),
        format!("{field} 的值 {value:?} 不在允许的取值 {allowed:?} 中"),
    )))
}

/// Decodes the picture up front so `Backup::unpack_avatars` can't fail on
/// the file's contents half way through writing them out.
fn check_avatar(data: &str) -> Check {
    if data.len() > limits::AVATAR_DATA_CHARS {
        return Err(reject(crate::tr!(
            "an avatar is larger than allowed".to_string(),
            "其中的头像超出了大小上限".to_string(),
        )));
    }
    let is_image = avatar::decode(data)
        .ok()
        .is_some_and(|bytes| avatar::Format::sniff(&bytes).is_some());
    if !is_image {
        return Err(reject(crate::tr!(
            "an avatar isn't a PNG, JPEG or WebP image".to_string(),
            "其中的头像不是 PNG、JPEG 或 WebP 图片".to_string(),
        )));
    }
    Ok(())
}

/// Like `check_avatar`, so `Backup::unpack_voice_samples` can't fail on the
/// file's contents half way through.
fn check_sample(data: &str) -> Check {
    if data.len() > limits::SAMPLE_DATA_CHARS {
        return Err(reject(crate::tr!(
            "a voice sample is larger than allowed".to_string(),
            "其中的音色样本超出了大小上限".to_string(),
        )));
    }
    let is_audio = avatar::decode(data)
        .ok()
        .is_some_and(|bytes| sample::Format::sniff(&bytes).is_some());
    if !is_audio {
        return Err(reject(crate::tr!(
            format!("a voice sample isn't a {} recording", sample::names()),
            format!("其中的音色样本不是 {} 音频", sample::names()),
        )));
    }
    Ok(())
}

fn at_most<T>(items: &[T], max: usize, field: &str) -> Check {
    if items.len() > max {
        return Err(reject(crate::tr!(
            format!(
                "it holds {} {field}, more than the {max} allowed",
                items.len()
            ),
            format!(
                "其中的 {field} 有 {} 个，超过了允许的 {max} 个",
                items.len()
            ),
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Backup {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    /// Informational: which build wrote the file, for when a restore
    /// misbehaves and the file is all there is to go on.
    pub app_version: String,
    #[serde(default)]
    pub characters: Vec<BackupCharacter>,
    /// Absent in a v1 file, and in one a user exported before this field
    /// existed; either way the restoring device keeps the settings it has.
    #[serde(default)]
    pub settings: Option<BackupSettings>,
    /// The kept sample of each custom voice a character uses. Absent from
    /// files written before samples were kept; an older build reading a
    /// newer file ignores it, and its characters keep their voices — only
    /// sharing one from there asks for the audio again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub voice_samples: Vec<BackupVoiceSample>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupVoiceSample {
    pub voice_id: String,
    /// The audio, base64-encoded like `avatar_data`. `None` until
    /// `Backup::embed_voice_samples` has read it, and dropped by then if it
    /// couldn't be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// The file the sample is in on this device: where `export` found it, or
    /// where `unpack_voice_samples` wrote it for `import` to point at. Never
    /// in the file, where it would name nothing.
    #[serde(skip)]
    pub file_name: Option<String>,
}

/// What the app needs to know to be usable on the other machine without a
/// setup session: the credentials it talks to DashScope with, and the
/// preferences the user tuned.
///
/// Every field is optional, and `None` means "leave this device's alone", so
/// a hand-trimmed file can bring the API key and nothing else. `api_key` is
/// the one that isn't a `settings` row — it lives in the OS keyring, and
/// `app::commands` moves it at both ends, because a keyring write can fail in
/// ways a row here can't and the user has to hear about it when it does.
///
/// `current_character_id` is deliberately not here: which character you had
/// open is about the seat you were sitting in, and a restore already picks a
/// current character for a device that has none.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BackupSettings {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    /// An empty string is the push-to-talk hotkey deliberately turned off,
    /// the same as in the row it comes from; `None` is "the file doesn't say".
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default)]
    pub vad_threshold: Option<f64>,
    #[serde(default)]
    pub vad_silence_ms: Option<i64>,
    /// `en` or `zh-CN`.
    #[serde(default)]
    pub ui_language: Option<String>,
}

/// A character with everything hanging off it. Nested rather than four
/// parallel tables, so the file reads as "here are my characters" and a
/// hand-trimmed one can't leave memories pointing at a character that isn't
/// in it.
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupCharacter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub avatar_path: Option<String>,
    /// The picture `avatar_path` names, base64-encoded. Absent in files
    /// written before avatars existed, and for a character without one; an
    /// older build reading a newer file ignores it and keeps a name that
    /// points at nothing, which the UI shows as no avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_data: Option<String>,
    pub language: String,
    pub persona: String,
    pub speech_habits: String,
    pub voice_kind: String,
    #[serde(default)]
    pub voice_id: Option<String>,
    #[serde(default)]
    pub voice_prompt: Option<String>,
    pub memory_enabled: bool,
    pub max_history_turns: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub memories: Vec<BackupMemory>,
    #[serde(default)]
    pub conversations: Vec<BackupConversation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupMemory {
    pub id: String,
    /// profile|fact|summary
    pub kind: String,
    pub content: String,
    pub salience: f64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupConversation {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// Whether it was summarized into the character's long-term memory.
    /// Absent from files written before conversations could be kept without
    /// being remembered; see `BackupConversation::is_memorized`.
    #[serde(default)]
    pub memorized: Option<bool>,
    #[serde(default)]
    pub messages: Vec<BackupMessage>,
}

impl BackupConversation {
    /// The file's own answer if it has one. Otherwise the file predates the
    /// field, from a build that only stored a conversation while recording it
    /// into memory and summarized each one as it ended — so the same rule as
    /// migration 0004: ended, and not by the dangling-row sweep, which stamps
    /// `ended_at` with the last message's time and never summarizes.
    fn is_memorized(&self) -> bool {
        self.memorized.unwrap_or_else(|| {
            let last_message = self.messages.iter().map(|m| m.created_at.as_str()).max();
            match (self.ended_at.as_deref(), last_message) {
                (Some(ended), Some(last)) => ended != last,
                _ => false,
            }
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupMessage {
    pub id: String,
    /// user|assistant
    pub role: String,
    pub text: String,
    /// Always null today — nothing writes it — but carried through so a
    /// backup stays a faithful copy of the row if that changes.
    #[serde(default)]
    pub audio_ms: Option<i64>,
    pub created_at: String,
}

/// What a file holds, for the one-line report Settings shows after a backup
/// or a restore.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Totals {
    pub characters: usize,
    pub memories: usize,
    pub conversations: usize,
    pub messages: usize,
}

impl Backup {
    pub fn totals(&self) -> Totals {
        Totals {
            characters: self.characters.len(),
            memories: self.characters.iter().map(|c| c.memories.len()).sum(),
            conversations: self.characters.iter().map(|c| c.conversations.len()).sum(),
            messages: self
                .characters
                .iter()
                .flat_map(|c| &c.conversations)
                .map(|v| v.messages.len())
                .sum(),
        }
    }

    /// Puts each character's picture into the file, read from `dir`. Kept
    /// apart from `export`, which only reads the database.
    pub fn embed_avatars(&mut self, dir: &Path) {
        for c in &mut self.characters {
            c.avatar_data = c
                .avatar_path
                .as_deref()
                .and_then(|name| avatar::read(dir, name))
                .map(|(bytes, _)| avatar::encode(&bytes));
        }
    }

    /// Writes the pictures the file carries into `dir` and points each
    /// character at its copy there, ready for `import`. Call after
    /// `validate`, which is what vouches for the pictures being images.
    ///
    /// A character the file has no picture for keeps its `avatar_path` only
    /// when this device has that file — restoring onto the machine the
    /// backup came from — and otherwise ends up without one.
    pub fn unpack_avatars(&mut self, dir: &Path) -> Result<(), String> {
        for c in &mut self.characters {
            c.avatar_path = match c.avatar_data.take() {
                Some(data) => Some(avatar::save(dir, &avatar::decode(&data)?)?),
                None => c
                    .avatar_path
                    .take()
                    .filter(|name| avatar::exists(dir, name)),
            };
        }
        Ok(())
    }

    /// Puts each voice sample `export` found into the file, read from `dir`,
    /// dropping any whose file has gone.
    pub fn embed_voice_samples(&mut self, dir: &Path) {
        for s in &mut self.voice_samples {
            s.data = s
                .file_name
                .as_deref()
                .and_then(|name| sample::read(dir, name))
                .map(|(bytes, _)| avatar::encode(&bytes));
        }
        self.voice_samples.retain(|s| s.data.is_some());
    }

    /// Writes the samples the file carries into `dir`, ready for `import` to
    /// record against their voices. Call after `validate`, which vouches for
    /// them being audio. The files a restored sample replaces are left for
    /// the next launch's sweep.
    pub fn unpack_voice_samples(&mut self, dir: &Path) -> Result<(), String> {
        for s in &mut self.voice_samples {
            s.file_name = match s.data.take() {
                Some(data) => Some(sample::save(dir, &avatar::decode(&data)?)?),
                None => None,
            };
        }
        Ok(())
    }

    /// Rejects a file this build can't be trusted to restore, before any of
    /// it reaches the database.
    pub fn check_compatible(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(crate::tr!(
                "This file isn't a VoiceChat backup",
                "这不是 VoiceChat 的备份文件",
            )
            .into());
        }
        if self.version > VERSION {
            return Err(crate::tr!(
                format!(
                    "This backup was written by a newer version of VoiceChat (format v{}) — update the app first",
                    self.version
                ),
                format!(
                    "该备份由更新版本的 VoiceChat 导出（格式 v{}），请先升级应用",
                    self.version
                ),
            ));
        }
        Ok(())
    }

    /// Checks the contents against `limits` and the closed value sets, so a
    /// file that is damaged or hostile is refused whole rather than half
    /// restored. Runs before `import` opens its transaction: rejecting here
    /// costs nothing, whereas failing part way through would leave the user
    /// wondering what did and didn't land.
    ///
    /// Deliberately strict rather than forgiving. Every value this app writes
    /// passes, so the only files this turns away are ones no user of it
    /// produced — and quietly clamping those would mean importing something
    /// other than what the file says, which is worse than saying no.
    pub fn validate(&self) -> Result<(), String> {
        at_most(&self.characters, limits::CHARACTERS, "characters")?;
        text(&self.exported_at, limits::TIMESTAMP_CHARS, "exported_at")?;
        text(&self.app_version, limits::NAME_CHARS, "app_version")?;

        for c in &self.characters {
            required(&c.id, limits::ID_CHARS, "a character id")?;
            required(&c.name, limits::NAME_CHARS, "a character name")?;
            one_of(&c.language, LANGUAGES, "a character's language")?;
            one_of(&c.voice_kind, VOICE_KINDS, "a character's voice_kind")?;
            text(&c.persona, limits::PROSE_CHARS, "a character's persona")?;
            text(
                &c.speech_habits,
                limits::PROSE_CHARS,
                "a character's speech_habits",
            )?;
            text(
                c.voice_id.as_deref().unwrap_or(""),
                limits::VOICE_ID_CHARS,
                "a voice_id",
            )?;
            text(
                c.voice_prompt.as_deref().unwrap_or(""),
                limits::PROSE_CHARS,
                "a voice_prompt",
            )?;
            text(
                c.avatar_path.as_deref().unwrap_or(""),
                limits::PATH_CHARS,
                "an avatar_path",
            )?;
            if let Some(data) = &c.avatar_data {
                check_avatar(data)?;
            }
            text(
                &c.created_at,
                limits::TIMESTAMP_CHARS,
                "a character's created_at",
            )?;
            text(
                &c.updated_at,
                limits::TIMESTAMP_CHARS,
                "a character's updated_at",
            )?;

            // Sent to the API as the session's history window, so an absurd
            // value is a request this app would never make on its own.
            if !(1..=limits::MAX_HISTORY_TURNS).contains(&c.max_history_turns) {
                return Err(reject(crate::tr!(
                    format!(
                        "max_history_turns is {}, outside the range 1-{}",
                        c.max_history_turns,
                        limits::MAX_HISTORY_TURNS
                    ),
                    format!(
                        "max_history_turns 的值 {} 超出了 1-{} 的范围",
                        c.max_history_turns,
                        limits::MAX_HISTORY_TURNS
                    ),
                )));
            }

            at_most(&c.memories, limits::MEMORIES_PER_CHARACTER, "memories")?;
            for m in &c.memories {
                required(&m.id, limits::ID_CHARS, "a memory id")?;
                one_of(&m.kind, MEMORY_KINDS, "a memory's kind")?;
                text(&m.content, limits::PROSE_CHARS, "a memory's content")?;
                text(
                    &m.updated_at,
                    limits::TIMESTAMP_CHARS,
                    "a memory's updated_at",
                )?;
                // Orders which memories get injected into the instructions.
                // NaN would make that ordering meaningless rather than merely
                // wrong, so it is ruled out along with out-of-range values.
                if !m.salience.is_finite() || !(0.0..=1.0).contains(&m.salience) {
                    return Err(reject(crate::tr!(
                        format!("a memory's salience is {}, outside 0.0-1.0", m.salience),
                        format!("记忆的 salience 值 {} 超出了 0.0-1.0 的范围", m.salience),
                    )));
                }
            }

            at_most(
                &c.conversations,
                limits::CONVERSATIONS_PER_CHARACTER,
                "conversations",
            )?;
            for v in &c.conversations {
                required(&v.id, limits::ID_CHARS, "a conversation id")?;
                text(
                    v.title.as_deref().unwrap_or(""),
                    limits::TITLE_CHARS,
                    "a conversation title",
                )?;
                text(
                    &v.started_at,
                    limits::TIMESTAMP_CHARS,
                    "a conversation's started_at",
                )?;
                text(
                    v.ended_at.as_deref().unwrap_or(""),
                    limits::TIMESTAMP_CHARS,
                    "a conversation's ended_at",
                )?;

                at_most(&v.messages, limits::MESSAGES_PER_CONVERSATION, "messages")?;
                for msg in &v.messages {
                    required(&msg.id, limits::ID_CHARS, "a message id")?;
                    one_of(&msg.role, MESSAGE_ROLES, "a message's role")?;
                    text(&msg.text, limits::MESSAGE_CHARS, "a message's text")?;
                    text(
                        &msg.created_at,
                        limits::TIMESTAMP_CHARS,
                        "a message's created_at",
                    )?;
                }
            }
        }

        // One per character's voice at most, in a file this app wrote.
        at_most(&self.voice_samples, limits::CHARACTERS, "voice samples")?;
        for s in &self.voice_samples {
            required(&s.voice_id, limits::VOICE_ID_CHARS, "a voice sample's voice_id")?;
            if let Some(data) = &s.data {
                check_sample(data)?;
            }
        }

        if let Some(settings) = &self.settings {
            settings.validate()?;
        }
        Ok(())
    }
}

impl BackupSettings {
    /// The workspace id and region get exactly the check
    /// `set_connection_settings` applies to what a user types, because they
    /// end up in the same place: interpolated into the hostname every request
    /// — API key attached — is sent to. A backup is not a way around it.
    fn validate(&self) -> Check {
        if let Some(key) = &self.api_key {
            required(key, limits::API_KEY_CHARS, "the API key")?;
        }

        if let Some(workspace_id) = self.workspace_id.as_deref() {
            let workspace_id = workspace_id.trim();
            if !workspace_id.is_empty() && !crate::dashscope::is_valid_workspace_id(workspace_id) {
                return Err(reject(crate::tr!(
                    format!("{workspace_id:?} isn't a Workspace ID"),
                    format!("{workspace_id:?} 不是有效的 Workspace ID"),
                )));
            }
        }

        if let Some(region) = self.region.as_deref() {
            let region = region.trim();
            if !region.is_empty() && !crate::dashscope::is_valid_region(region) {
                return Err(reject(crate::tr!(
                    format!("the region is {region:?}, which this build doesn't know"),
                    format!("地域 {region:?} 不是本版本已知的取值"),
                )));
            }
        }

        if let Some(hotkey) = &self.hotkey {
            text(hotkey, limits::HOTKEY_CHARS, "the hotkey")?;
        }

        if let Some(tag) = self.ui_language.as_deref() {
            one_of(tag, UI_LANGUAGE_TAGS, "the display language")?;
        }

        if let Some(threshold) = self.vad_threshold {
            if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
                return Err(reject(crate::tr!(
                    format!("the speech-detection threshold is {threshold}, outside 0.0-1.0"),
                    format!("语音检测灵敏度 {threshold} 超出了 0.0-1.0 的范围"),
                )));
            }
        }

        if let Some(ms) = self.vad_silence_ms {
            if !(limits::MIN_VAD_SILENCE_MS..=limits::MAX_VAD_SILENCE_MS).contains(&ms) {
                return Err(reject(crate::tr!(
                    format!(
                        "the end-of-speech silence is {ms} ms, outside {}-{} ms",
                        limits::MIN_VAD_SILENCE_MS,
                        limits::MAX_VAD_SILENCE_MS
                    ),
                    format!(
                        "停顿判定时长 {ms} ms 超出了 {}-{} ms 的范围",
                        limits::MIN_VAD_SILENCE_MS,
                        limits::MAX_VAD_SILENCE_MS
                    ),
                )));
            }
        }
        Ok(())
    }
}

fn export_memories(conn: &Connection, character_id: &str) -> rusqlite::Result<Vec<BackupMemory>> {
    Ok(memory::list(conn, character_id)?
        .into_iter()
        .map(|m| BackupMemory {
            id: m.id,
            kind: m.kind,
            content: m.content,
            salience: m.salience,
            updated_at: m.updated_at,
        })
        .collect())
}

/// Only conversations that hold something, matching what the Chat tab's
/// history list shows. Every connect opens a row, so exporting all of them
/// would pad the file with sessions nobody spoke in — including the one the
/// live session may have open right now.
fn export_conversations(
    conn: &Connection,
    character_id: &str,
) -> rusqlite::Result<Vec<BackupConversation>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, started_at, ended_at, memorized FROM conversations c \
         WHERE c.character_id = ?1 \
         AND EXISTS (SELECT 1 FROM messages WHERE conversation_id = c.id) \
         ORDER BY c.started_at ASC",
    )?;
    let rows = stmt.query_map(params![character_id], |row| {
        Ok(BackupConversation {
            id: row.get("id")?,
            title: row.get("title")?,
            started_at: row.get("started_at")?,
            ended_at: row.get("ended_at")?,
            memorized: Some(row.get("memorized")?),
            messages: Vec::new(),
        })
    })?;
    let mut conversations: Vec<BackupConversation> = rows.collect::<rusqlite::Result<_>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, role, text, audio_ms, created_at FROM messages \
         WHERE conversation_id = ?1 ORDER BY created_at ASC",
    )?;
    for conversation in &mut conversations {
        let rows = stmt.query_map(params![conversation.id], |row| {
            Ok(BackupMessage {
                id: row.get("id")?,
                role: row.get("role")?,
                text: row.get("text")?,
                audio_ms: row.get("audio_ms")?,
                created_at: row.get("created_at")?,
            })
        })?;
        conversation.messages = rows.collect::<rusqlite::Result<_>>()?;
    }
    Ok(conversations)
}

/// Reads back the `settings` rows. Values this build doesn't recognise are
/// dropped rather than carried: a `vad_threshold` that won't parse is a row
/// the app is already ignoring in favour of the default, and writing it into
/// a backup would only export a bug to the next machine.
fn export_settings(conn: &Connection, api_key: Option<String>) -> rusqlite::Result<BackupSettings> {
    Ok(BackupSettings {
        api_key,
        workspace_id: db::get_setting(conn, "workspace_id")?,
        region: db::get_setting(conn, "region")?,
        hotkey: db::get_setting(conn, "hotkey")?,
        vad_threshold: db::get_setting(conn, "vad_threshold")?.and_then(|v| v.parse().ok()),
        vad_silence_ms: db::get_setting(conn, "vad_silence_ms")?.and_then(|v| v.parse().ok()),
        ui_language: db::get_setting(conn, "ui_language")?,
    })
}

/// `api_key` is handed in rather than read here: it lives in the OS keyring,
/// not this database, and whether the user wanted it in the file at all is
/// something only the caller knows.
pub fn export(conn: &Connection, api_key: Option<String>) -> rusqlite::Result<Backup> {
    let mut characters = Vec::new();
    let mut voice_samples: Vec<BackupVoiceSample> = Vec::new();
    for c in character::list(conn)? {
        // Only the voices characters use: the rest of the account's voices
        // aren't anyone's to share from the other machine, and a sample can
        // run to megabytes. A preset voice has no sample, and never a row.
        if let Some(voice_id) = c.voice_id.as_deref().filter(|_| c.voice_kind != "preset") {
            if !voice_samples.iter().any(|s| s.voice_id == voice_id) {
                if let Some(file_name) = voice_sample::get(conn, voice_id)? {
                    voice_samples.push(BackupVoiceSample {
                        voice_id: voice_id.to_string(),
                        data: None,
                        file_name: Some(file_name),
                    });
                }
            }
        }
        characters.push(BackupCharacter {
            memories: export_memories(conn, &c.id)?,
            conversations: export_conversations(conn, &c.id)?,
            id: c.id,
            name: c.name,
            avatar_path: c.avatar_path,
            avatar_data: None,
            language: c.language,
            persona: c.persona,
            speech_habits: c.speech_habits,
            voice_kind: c.voice_kind,
            voice_id: c.voice_id,
            voice_prompt: c.voice_prompt,
            memory_enabled: c.memory_enabled,
            max_history_turns: c.max_history_turns,
            created_at: c.created_at,
            updated_at: c.updated_at,
        });
    }
    Ok(Backup {
        format: FORMAT.to_string(),
        version: VERSION,
        exported_at: Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        characters,
        settings: Some(export_settings(conn, api_key)?),
        voice_samples,
    })
}

/// The result of a restore: what came out of the file, and how much of it
/// was new here.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ImportSummary {
    pub totals: Totals,
    /// Characters the file brought that this device didn't have. The rest
    /// already existed and were merged into.
    pub new_characters: usize,
    /// Whether the file carried settings, and whether one of them was the API
    /// key. Both are worth telling the user about after a restore: they
    /// change what this install connects *as*, not just what it holds.
    ///
    /// `api_key_restored` is left for the caller to set — the key does not go
    /// through this database, and "the file had one" is not the same claim as
    /// "the keyring took it".
    pub settings_restored: bool,
    pub api_key_restored: bool,
}

/// Merges a backup into the database, keeping whatever is already here.
///
/// Rows are matched by id — the same UUIDs the exporting device wrote — so
/// restoring a file twice, or restoring onto the machine it came from,
/// updates in place instead of duplicating everything. Where a row exists on
/// both sides the backup wins: it is what the user asked to restore, and for
/// rows that share an origin the two are the same content anyway.
///
/// Settings are the exception to the merge: the file's values overwrite this
/// device's rather than yielding to them, because the whole reason to restore
/// them is for the new machine to behave like the old one. Only the fields
/// the file actually carries are touched.
///
/// It all happens in one transaction, so a file that turns out to be
/// truncated half way through leaves the database as it was rather than
/// half-restored. Upserts rather than `INSERT OR REPLACE`: replacing a
/// conversation row would delete it first, and `ON DELETE CASCADE` would
/// take the messages already stored under it along with it.
pub fn import(conn: &mut Connection, backup: &Backup) -> rusqlite::Result<ImportSummary> {
    let tx = conn.transaction()?;
    let mut new_characters = 0;

    for c in &backup.characters {
        let existed: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM characters WHERE id = ?1)",
            params![c.id],
            |row| row.get(0),
        )?;
        if !existed {
            new_characters += 1;
        }

        tx.execute(
            "INSERT INTO characters (
                id, name, avatar_path, language, persona, speech_habits,
                voice_kind, voice_id, voice_prompt, memory_enabled, max_history_turns,
                created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                avatar_path = excluded.avatar_path,
                language = excluded.language,
                persona = excluded.persona,
                speech_habits = excluded.speech_habits,
                voice_kind = excluded.voice_kind,
                voice_id = excluded.voice_id,
                voice_prompt = excluded.voice_prompt,
                memory_enabled = excluded.memory_enabled,
                max_history_turns = excluded.max_history_turns,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at",
            params![
                c.id,
                c.name,
                c.avatar_path,
                c.language,
                c.persona,
                c.speech_habits,
                c.voice_kind,
                c.voice_id,
                c.voice_prompt,
                c.memory_enabled as i64,
                c.max_history_turns,
                c.created_at,
                c.updated_at,
            ],
        )?;

        for m in &c.memories {
            tx.execute(
                "INSERT INTO memories (id, character_id, kind, content, salience, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(id) DO UPDATE SET
                    character_id = excluded.character_id,
                    kind = excluded.kind,
                    content = excluded.content,
                    salience = excluded.salience,
                    updated_at = excluded.updated_at",
                params![m.id, c.id, m.kind, m.content, m.salience, m.updated_at],
            )?;
        }

        for v in &c.conversations {
            tx.execute(
                "INSERT INTO conversations (id, character_id, started_at, ended_at, title, memorized)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(id) DO UPDATE SET
                    character_id = excluded.character_id,
                    started_at = excluded.started_at,
                    ended_at = excluded.ended_at,
                    title = excluded.title,
                    memorized = excluded.memorized",
                params![v.id, c.id, v.started_at, v.ended_at, v.title, v.is_memorized()],
            )?;

            for msg in &v.messages {
                tx.execute(
                    "INSERT INTO messages (id, conversation_id, role, text, audio_ms, created_at)
                     VALUES (?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(id) DO UPDATE SET
                        conversation_id = excluded.conversation_id,
                        role = excluded.role,
                        text = excluded.text,
                        audio_ms = excluded.audio_ms,
                        created_at = excluded.created_at",
                    params![
                        msg.id,
                        v.id,
                        msg.role,
                        msg.text,
                        msg.audio_ms,
                        msg.created_at,
                    ],
                )?;
            }
        }
    }

    // Samples `unpack_voice_samples` wrote out. The file wins over a sample
    // this device already had for the voice, as it does for everything else.
    for s in &backup.voice_samples {
        if let Some(file_name) = &s.file_name {
            voice_sample::set(&tx, &s.voice_id, file_name)?;
        }
    }

    // In the same transaction as the characters, minus the two that reach
    // outside the database: the API key (OS keyring) and the hotkey (the OS's
    // global shortcut table, where registering can fail because another app
    // already holds the combo). `app::commands::import_backup` applies those
    // once this has committed.
    if let Some(s) = &backup.settings {
        if let Some(v) = &s.workspace_id {
            db::set_setting(&tx, "workspace_id", v.trim())?;
        }
        if let Some(v) = &s.region {
            db::set_setting(&tx, "region", v.trim())?;
        }
        if let Some(v) = s.vad_threshold {
            db::set_setting(&tx, "vad_threshold", &v.to_string())?;
        }
        if let Some(v) = s.vad_silence_ms {
            db::set_setting(&tx, "vad_silence_ms", &v.to_string())?;
        }
        if let Some(v) = &s.ui_language {
            db::set_setting(&tx, "ui_language", v)?;
        }
    }

    tx.commit()?;
    Ok(ImportSummary {
        totals: backup.totals(),
        new_characters,
        settings_restored: backup.settings.is_some(),
        api_key_restored: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{character::CharacterInput, db, message};

    /// A migrated, empty database in a throwaway file. On-disk rather than
    /// `:memory:` because `db::open` — the only thing that knows how to
    /// apply the migrations — takes a path.
    struct TempDb {
        path: std::path::PathBuf,
        conn: Connection,
    }

    impl TempDb {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("voicechat-backup-test-{}.db", uuid::Uuid::new_v4()));
            let conn = db::open(&path).expect("open");
            Self { path, conn }
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("voicechat-backup-avatars-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn seed(conn: &Connection) -> String {
        let c = character::create(
            conn,
            CharacterInput::default_new("Nia", "curious and warm", "short sentences"),
        )
        .expect("character");
        memory::create(conn, &c.id, "fact", "likes oolong tea", 0.8).expect("memory");
        memory::create(conn, &c.id, "summary", "talked about moving house", 1.0).expect("memory");

        let conversation = message::start_conversation(conn, &c.id).expect("conversation");
        message::insert_message(conn, &conversation.id, "user", "hello").expect("message");
        message::insert_message(conn, &conversation.id, "assistant", "hi there").expect("message");
        message::set_title(conn, &conversation.id, "First chat").expect("title");
        message::end_conversation(conn, &conversation.id).expect("end");
        message::mark_memorized(conn, &conversation.id).expect("memorized");

        // An empty conversation, of the kind every connect opens: it must
        // not travel with the backup.
        message::start_conversation(conn, &c.id).expect("empty conversation");
        c.id
    }

    /// The whole point of the feature: what one device exports is what
    /// another device ends up with.
    #[test]
    fn round_trips_through_json_onto_a_fresh_device() {
        let source = TempDb::new();
        let character_id = seed(&source.conn);

        let exported = export(&source.conn, None).expect("export");
        let json = serde_json::to_string(&exported).expect("serialize");
        let parsed: Backup = serde_json::from_str(&json).expect("deserialize");
        parsed.check_compatible().expect("compatible");

        let mut target = TempDb::new();
        let summary = import(&mut target.conn, &parsed).expect("import");
        assert_eq!(summary.new_characters, 1);
        assert_eq!(
            (
                summary.totals.characters,
                summary.totals.memories,
                summary.totals.conversations,
                summary.totals.messages
            ),
            (1, 2, 1, 2),
            "the empty conversation should have been left behind"
        );

        let restored = character::get(&target.conn, &character_id)
            .expect("get")
            .expect("character restored under its original id");
        assert_eq!(restored.name, "Nia");
        assert_eq!(restored.persona, "curious and warm");

        let memories = memory::list(&target.conn, &character_id).expect("memories");
        assert_eq!(memories.len(), 2);
        assert!(memories.iter().any(|m| m.content == "likes oolong tea"));

        let conversations =
            message::list_conversations(&target.conn, &character_id).expect("conversations");
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].title.as_deref(), Some("First chat"));
        assert!(
            conversations[0].memorized,
            "the history list's memory mark travels with the conversation"
        );
        let messages =
            message::list_messages(&target.conn, &conversations[0].id).expect("messages");
        assert_eq!(
            messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["hello", "hi there"]
        );
    }

    /// Restoring twice — or restoring onto the machine the file came from —
    /// updates in place instead of duplicating, and leaves data that was
    /// only on this device alone.
    #[test]
    fn re_importing_merges_instead_of_duplicating() {
        let mut db = TempDb::new();
        let character_id = seed(&db.conn);
        let backup = export(&db.conn, None).expect("export");

        let local_only = character::create(
            &db.conn,
            CharacterInput::default_new("Local", "only on this device", ""),
        )
        .expect("local character");
        // A character the backup also has, edited since it was taken.
        character::update(
            &db.conn,
            &character_id,
            CharacterInput::default_new("Renamed", "edited after the backup", ""),
        )
        .expect("edit");

        let summary = import(&mut db.conn, &backup).expect("re-import");
        assert_eq!(summary.new_characters, 0);

        let characters = character::list(&db.conn).expect("list");
        assert_eq!(characters.len(), 2, "nothing was duplicated");
        assert!(
            characters.iter().any(|c| c.id == local_only.id),
            "a character only this device had survives the restore"
        );
        let restored = character::get(&db.conn, &character_id)
            .expect("get")
            .expect("still there");
        assert_eq!(
            restored.name, "Nia",
            "the backup's version wins over the local edit"
        );

        let conversations =
            message::list_conversations(&db.conn, &character_id).expect("conversations");
        assert_eq!(conversations.len(), 1);
        assert_eq!(
            message::list_messages(&db.conn, &conversations[0].id)
                .expect("messages")
                .len(),
            2,
            "upserting the conversation must not cascade its messages away"
        );
        assert_eq!(
            memory::list(&db.conn, &character_id)
                .expect("memories")
                .len(),
            2
        );
    }

    /// A file from before `memorized` existed still gets its marks right: a
    /// conversation that ended properly was summarized on the way out, one
    /// the dangling-row sweep closed was not.
    #[test]
    fn legacy_files_infer_the_memory_mark() {
        let conversation = |ended_at: Option<&str>| BackupConversation {
            id: "v1".into(),
            title: None,
            started_at: "2026-01-01T10:00:00+00:00".into(),
            ended_at: ended_at.map(Into::into),
            memorized: None,
            messages: vec![BackupMessage {
                id: "msg1".into(),
                role: "user".into(),
                text: "hello".into(),
                audio_ms: None,
                created_at: "2026-01-01T10:01:00+00:00".into(),
            }],
        };
        assert!(conversation(Some("2026-01-01T10:05:00+00:00")).is_memorized());
        assert!(
            !conversation(Some("2026-01-01T10:01:00+00:00")).is_memorized(),
            "closed by the sweep, never summarized"
        );
        assert!(!conversation(None).is_memorized());

        let mut explicit = conversation(Some("2026-01-01T10:01:00+00:00"));
        explicit.memorized = Some(true);
        assert!(explicit.is_memorized(), "the file's own answer wins");
    }

    /// The other half of "seamless on the next machine": the settings that
    /// say which account to talk to and how, carried across with everything
    /// else so nothing has to be retyped.
    #[test]
    fn settings_travel_with_the_backup() {
        let source = TempDb::new();
        seed(&source.conn);
        for (key, value) in [
            ("workspace_id", "llm-abc123"),
            ("region", "ap-southeast-1"),
            ("hotkey", "Ctrl+Alt+K"),
            ("vad_threshold", "0.35"),
            ("vad_silence_ms", "1200"),
            ("ui_language", "zh-CN"),
        ] {
            db::set_setting(&source.conn, key, value).expect("setting");
        }

        let exported = export(&source.conn, Some("sk-not-a-real-key".into())).expect("export");
        let json = serde_json::to_string(&exported).expect("serialize");
        let parsed: Backup = serde_json::from_str(&json).expect("deserialize");
        parsed
            .validate()
            .expect("its own settings must import back");
        let settings = parsed.settings.as_ref().expect("settings");
        assert_eq!(settings.api_key.as_deref(), Some("sk-not-a-real-key"));

        let mut target = TempDb::new();
        let summary = import(&mut target.conn, &parsed).expect("import");
        assert!(summary.settings_restored);
        assert!(
            !summary.api_key_restored,
            "the keyring is the caller's to write, so this stays false here"
        );

        let stored = |key: &str| db::get_setting(&target.conn, key).expect("get");
        assert_eq!(stored("workspace_id").as_deref(), Some("llm-abc123"));
        assert_eq!(stored("region").as_deref(), Some("ap-southeast-1"));
        assert_eq!(stored("vad_threshold").as_deref(), Some("0.35"));
        assert_eq!(stored("vad_silence_ms").as_deref(), Some("1200"));
        assert_eq!(stored("ui_language").as_deref(), Some("zh-CN"));
        // Carried in the file, but only `import_backup` can apply it: it has
        // to be registered with the OS before it is worth storing.
        assert_eq!(settings.hotkey.as_deref(), Some("Ctrl+Alt+K"));
        assert_eq!(stored("hotkey"), None);
    }

    /// A file from before settings were part of the format — or one someone
    /// stripped them out of — must not blank the settings of the device it
    /// lands on.
    #[test]
    fn a_file_without_settings_leaves_this_device_configured() {
        let mut db = TempDb::new();
        db::set_setting(&db.conn, "workspace_id", "llm-mine").expect("setting");
        db::set_setting(&db.conn, "region", "cn-beijing").expect("setting");

        let mut v1 = valid_backup();
        v1.version = 1;
        v1.settings = None;
        v1.check_compatible().expect("a v1 file still restores");
        v1.validate().expect("valid");

        let summary = import(&mut db.conn, &v1).expect("import");
        assert!(!summary.settings_restored);
        assert_eq!(
            db::get_setting(&db.conn, "workspace_id")
                .expect("get")
                .as_deref(),
            Some("llm-mine")
        );
        assert_eq!(
            db::get_setting(&db.conn, "region").expect("get").as_deref(),
            Some("cn-beijing")
        );
    }

    /// Settings decide where the API key gets sent, so a file's version of
    /// them is checked exactly as strictly as something typed into Settings.
    #[test]
    fn rejects_settings_it_would_have_to_connect_with() {
        fn with(edit: impl FnOnce(&mut BackupSettings)) -> Backup {
            let mut b = valid_backup();
            edit(b.settings.as_mut().expect("the fixture has settings"));
            b
        }

        assert!(
            with(|s| s.workspace_id = Some("evil.com/#".into()))
                .validate()
                .is_err(),
            "a workspace id that is really a host"
        );
        assert!(
            with(|s| s.region = Some("us-east-1".into()))
                .validate()
                .is_err(),
            "a region this build has no endpoint for"
        );
        assert!(
            with(|s| s.ui_language = Some("de".into()))
                .validate()
                .is_err(),
            "a display language with no catalogue"
        );
        assert!(
            with(|s| s.api_key = Some("   ".into())).validate().is_err(),
            "a blank API key, which would overwrite a working one with nothing"
        );
        assert!(
            with(|s| s.api_key = Some("k".repeat(limits::API_KEY_CHARS + 1)))
                .validate()
                .is_err(),
            "an API key no keyring should be asked to hold"
        );

        for threshold in [f64::NAN, -0.1, 1.5] {
            assert!(
                with(move |s| s.vad_threshold = Some(threshold))
                    .validate()
                    .is_err(),
                "threshold {threshold}"
            );
        }
        for ms in [0, limits::MAX_VAD_SILENCE_MS + 1] {
            assert!(
                with(move |s| s.vad_silence_ms = Some(ms))
                    .validate()
                    .is_err(),
                "silence {ms} ms"
            );
        }

        // The empty string is not a rejected value in either place: it is how
        // "no workspace", "no region" and "hotkey off" are stored.
        assert!(
            with(|s| s.workspace_id = Some(String::new()))
                .validate()
                .is_ok()
        );
        assert!(with(|s| s.region = Some(String::new())).validate().is_ok());
        assert!(with(|s| s.hotkey = Some(String::new())).validate().is_ok());
    }

    /// A minimal file that passes, as the starting point for tests that
    /// break exactly one thing about it.
    fn valid_backup() -> Backup {
        Backup {
            format: FORMAT.into(),
            version: VERSION,
            exported_at: Utc::now().to_rfc3339(),
            app_version: "0.1.0".into(),
            characters: vec![BackupCharacter {
                id: "c1".into(),
                name: "Nia".into(),
                avatar_path: None,
                avatar_data: None,
                language: "auto".into(),
                persona: "curious and warm".into(),
                speech_habits: "short sentences".into(),
                voice_kind: "preset".into(),
                voice_id: Some("longanqian".into()),
                voice_prompt: None,
                memory_enabled: true,
                max_history_turns: 20,
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
                memories: vec![BackupMemory {
                    id: "m1".into(),
                    kind: "fact".into(),
                    content: "likes oolong tea".into(),
                    salience: 0.5,
                    updated_at: Utc::now().to_rfc3339(),
                }],
                conversations: vec![BackupConversation {
                    id: "v1".into(),
                    title: Some("First chat".into()),
                    started_at: Utc::now().to_rfc3339(),
                    ended_at: None,
                    memorized: Some(true),
                    messages: vec![BackupMessage {
                        id: "msg1".into(),
                        role: "user".into(),
                        text: "hello".into(),
                        audio_ms: None,
                        created_at: Utc::now().to_rfc3339(),
                    }],
                }],
            }],
            settings: Some(BackupSettings {
                api_key: Some("sk-not-a-real-key".into()),
                workspace_id: Some("llm-abc123".into()),
                region: Some("cn-beijing".into()),
                hotkey: Some("Ctrl+Shift+Space".into()),
                vad_threshold: Some(0.5),
                vad_silence_ms: Some(800),
                ui_language: Some("zh-CN".into()),
            }),
            voice_samples: Vec::new(),
        }
    }

    /// The limits have to be set where real data fits under them, or the
    /// feature breaks for the people it exists for. This is the check that
    /// keeps the two honest.
    #[test]
    fn a_real_export_passes_its_own_validation() {
        let db = TempDb::new();
        seed(&db.conn);
        let exported = export(&db.conn, Some("sk-not-a-real-key".into())).expect("export");
        exported.check_compatible().expect("compatible");
        exported
            .validate()
            .expect("a file this app wrote must import back");

        valid_backup().validate().expect("the fixture is valid");
    }

    #[test]
    fn rejects_values_outside_the_closed_sets() {
        let mut b = valid_backup();
        b.characters[0].language = "'; DROP TABLE".into();
        assert!(b.validate().is_err(), "language");

        let mut b = valid_backup();
        b.characters[0].voice_kind = "anything".into();
        assert!(b.validate().is_err(), "voice_kind");

        let mut b = valid_backup();
        b.characters[0].memories[0].kind = "system".into();
        assert!(b.validate().is_err(), "memory kind");

        // The one that would change how a transcript is attributed when the
        // conversation is fed back to the summarizer.
        let mut b = valid_backup();
        b.characters[0].conversations[0].messages[0].role = "system".into();
        assert!(b.validate().is_err(), "message role");
    }

    #[test]
    fn rejects_oversized_content() {
        let mut b = valid_backup();
        b.characters[0].persona = "x".repeat(limits::PROSE_CHARS + 1);
        assert!(b.validate().is_err(), "persona length");

        let mut b = valid_backup();
        b.characters[0].name = String::new();
        assert!(b.validate().is_err(), "an empty name");

        let mut b = valid_backup();
        b.characters = std::iter::repeat_with(|| {
            let mut c = valid_backup()
                .characters
                .pop()
                .expect("the fixture has one");
            c.id = uuid::Uuid::new_v4().to_string();
            c
        })
        .take(limits::CHARACTERS + 1)
        .collect();
        assert!(b.validate().is_err(), "too many characters");
    }

    #[test]
    fn rejects_numbers_outside_their_range() {
        for turns in [0, -1, limits::MAX_HISTORY_TURNS + 1] {
            let mut b = valid_backup();
            b.characters[0].max_history_turns = turns;
            assert!(b.validate().is_err(), "max_history_turns {turns}");
        }

        for salience in [f64::NAN, f64::INFINITY, -0.5, 1.5] {
            let mut b = valid_backup();
            b.characters[0].memories[0].salience = salience;
            assert!(b.validate().is_err(), "salience {salience}");
        }
    }

    #[test]
    fn avatars_travel_inside_the_file() {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        let from_dir = TempDir::new();
        let to_dir = TempDir::new();

        let source = TempDb::new();
        let id = seed(&source.conn);
        let name = avatar::save(&from_dir.0, PNG).expect("save avatar");
        character::set_avatar(&source.conn, &id, Some(&name)).expect("set avatar");

        let mut exported = export(&source.conn, None).expect("export");
        exported.embed_avatars(&from_dir.0);
        let json = serde_json::to_string(&exported).expect("serialize");

        let mut restored: Backup = serde_json::from_str(&json).expect("parse");
        restored.validate().expect("valid");
        restored.unpack_avatars(&to_dir.0).expect("unpack");
        let mut target = TempDb::new();
        import(&mut target.conn, &restored).expect("import");

        let landed = character::get(&target.conn, &id)
            .expect("get")
            .and_then(|c| c.avatar_path)
            .expect("the character came with its avatar");
        assert_eq!(
            avatar::read(&to_dir.0, &landed).map(|(bytes, _)| bytes),
            Some(PNG.to_vec())
        );
    }

    /// What makes sharing from the restored device as easy as from this one.
    #[test]
    fn voice_samples_travel_inside_the_file() {
        const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt ";
        let from_dir = TempDir::new();
        let to_dir = TempDir::new();

        let source = TempDb::new();
        let id = seed(&source.conn);
        let mut cloned = CharacterInput::default_new("Nia", "curious and warm", "");
        cloned.voice_kind = "cloned".into();
        cloned.voice_id = Some("qwen-voice-nia".into());
        character::update(&source.conn, &id, cloned).expect("give it a cloned voice");
        let name = sample::save(&from_dir.0, WAV).expect("save sample");
        voice_sample::set(&source.conn, "qwen-voice-nia", &name).expect("record sample");
        // A voice no character uses stays behind.
        let unused = sample::save(&from_dir.0, WAV).expect("save sample");
        voice_sample::set(&source.conn, "qwen-voice-unused", &unused).expect("record sample");

        let mut exported = export(&source.conn, None).expect("export");
        exported.embed_voice_samples(&from_dir.0);
        let json = serde_json::to_string(&exported).expect("serialize");
        assert!(!json.contains(&name), "a file name means nothing elsewhere");

        let mut restored: Backup = serde_json::from_str(&json).expect("parse");
        assert_eq!(restored.voice_samples.len(), 1);
        restored.validate().expect("valid");
        restored.unpack_voice_samples(&to_dir.0).expect("unpack");
        let mut target = TempDb::new();
        import(&mut target.conn, &restored).expect("import");

        let landed = voice_sample::get(&target.conn, "qwen-voice-nia")
            .expect("get")
            .expect("the voice came with its sample");
        assert_eq!(
            sample::read(&to_dir.0, &landed).map(|(bytes, _)| bytes),
            Some(WAV.to_vec())
        );
        assert_eq!(voice_sample::get(&target.conn, "qwen-voice-unused").expect("get"), None);
    }

    #[test]
    fn rejects_a_voice_sample_that_isnt_audio() {
        let mut b = valid_backup();
        b.voice_samples.push(BackupVoiceSample {
            voice_id: "v".into(),
            data: Some(avatar::encode(b"\x89PNG\r\n\x1a\n")),
            file_name: None,
        });
        assert!(b.validate().is_err());
    }

    #[test]
    fn a_name_without_its_picture_is_dropped() {
        let dir = TempDir::new();
        let mut b = valid_backup();
        b.characters[0].avatar_path = Some("0f8fad5b-d9cb-469f-a165-70867728950e.png".into());
        b.unpack_avatars(&dir.0).expect("unpack");
        assert_eq!(b.characters[0].avatar_path, None);
    }

    #[test]
    fn rejects_an_avatar_that_isnt_an_image() {
        let mut b = valid_backup();
        b.characters[0].avatar_data = Some(avatar::encode(b"<svg onload=alert(1)>"));
        assert!(b.validate().is_err(), "not an image");

        let mut b = valid_backup();
        b.characters[0].avatar_data = Some("not base64!".into());
        assert!(b.validate().is_err(), "not base64");
    }

    #[test]
    fn rejects_files_it_cannot_restore() {
        let mut wrong = Backup {
            format: "something-else".into(),
            version: 1,
            exported_at: String::new(),
            app_version: String::new(),
            characters: Vec::new(),
            settings: None,
            voice_samples: Vec::new(),
        };
        assert!(wrong.check_compatible().is_err());

        wrong.format = FORMAT.into();
        wrong.version = VERSION + 1;
        assert!(wrong.check_compatible().is_err());

        wrong.version = VERSION;
        assert!(wrong.check_compatible().is_ok());
    }
}
