use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub avatar_path: Option<String>,
    pub language: String,
    pub persona: String,
    pub speech_habits: String,
    pub voice_kind: String,
    pub voice_id: Option<String>,
    pub voice_prompt: Option<String>,
    pub memory_enabled: bool,
    pub max_history_turns: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterInput {
    pub name: String,
    pub avatar_path: Option<String>,
    pub language: String,
    pub persona: String,
    pub speech_habits: String,
    pub voice_kind: String,
    pub voice_id: Option<String>,
    pub voice_prompt: Option<String>,
    pub memory_enabled: bool,
    pub max_history_turns: i64,
}

impl CharacterInput {
    pub fn default_new(name: &str, persona: &str, speech_habits: &str) -> Self {
        Self {
            name: name.to_string(),
            avatar_path: None,
            language: "auto".into(),
            persona: persona.to_string(),
            speech_habits: speech_habits.to_string(),
            voice_kind: "preset".into(),
            voice_id: Some("longanqian".into()),
            voice_prompt: None,
            memory_enabled: true,
            max_history_turns: 20,
        }
    }
}

/// The longest name a character may have, in display columns: a Chinese
/// character (or any other full-width one) counts two, everything else one.
/// So 12 Chinese characters or 24 Latin letters — about what the Chat
/// header fits on one line beside the memory toggle at the window's minimum
/// width. Mirrored by `NAME_MAX_WIDTH` in the character editor.
pub const NAME_MAX_WIDTH: usize = 24;

/// East Asian wide and full-width ranges, plus the emoji blocks, which
/// render just as wide. Mirrored by `isWide` in the character editor.
fn is_wide(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    )
}

pub fn name_width(name: &str) -> usize {
    name.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

/// The name as it should be stored — trimmed — or why it can't be.
pub fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::tr!("Please give the character a name", "请填写角色名字").into());
    }
    if name_width(name) > NAME_MAX_WIDTH {
        return Err(crate::tr!(
            "Names can be up to 12 Chinese characters or 24 Latin letters",
            "名字最多 12 个汉字或 24 个英文字母",
        )
        .into());
    }
    Ok(name.to_string())
}

fn row_to_character(row: &rusqlite::Row) -> rusqlite::Result<Character> {
    Ok(Character {
        id: row.get("id")?,
        name: row.get("name")?,
        avatar_path: row.get("avatar_path")?,
        language: row.get("language")?,
        persona: row.get("persona")?,
        speech_habits: row.get("speech_habits")?,
        voice_kind: row.get("voice_kind")?,
        voice_id: row.get("voice_id")?,
        voice_prompt: row.get("voice_prompt")?,
        memory_enabled: row.get::<_, i64>("memory_enabled")? != 0,
        max_history_turns: row.get("max_history_turns")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const COLUMNS: &str = "id, name, avatar_path, language, persona, speech_habits, \
    voice_kind, voice_id, voice_prompt, memory_enabled, max_history_turns, created_at, updated_at";

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Character>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM characters ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map([], row_to_character)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Character>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM characters WHERE id = ?1"),
        params![id],
        row_to_character,
    )
    .optional()
}

pub fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM characters", [], |row| row.get(0))
}

pub fn create(conn: &Connection, input: CharacterInput) -> rusqlite::Result<Character> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO characters (
            id, name, avatar_path, language, persona, speech_habits,
            voice_kind, voice_id, voice_prompt, memory_enabled, max_history_turns,
            created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            id,
            input.name,
            input.avatar_path,
            input.language,
            input.persona,
            input.speech_habits,
            input.voice_kind,
            input.voice_id,
            input.voice_prompt,
            input.memory_enabled as i64,
            input.max_history_turns,
            now,
            now,
        ],
    )?;
    get(conn, &id).map(|c| c.expect("just inserted"))
}

pub fn update(
    conn: &Connection,
    id: &str,
    input: CharacterInput,
) -> rusqlite::Result<Option<Character>> {
    let now = Utc::now().to_rfc3339();
    let affected = conn.execute(
        "UPDATE characters SET
            name = ?1, avatar_path = ?2, language = ?3, persona = ?4, speech_habits = ?5,
            voice_kind = ?6, voice_id = ?7, voice_prompt = ?8,
            memory_enabled = ?9, max_history_turns = ?10, updated_at = ?11
        WHERE id = ?12",
        params![
            input.name,
            input.avatar_path,
            input.language,
            input.persona,
            input.speech_habits,
            input.voice_kind,
            input.voice_id,
            input.voice_prompt,
            input.memory_enabled as i64,
            input.max_history_turns,
            now,
            id,
        ],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    get(conn, id)
}

/// Changes the picture alone, leaving every field the live session reads
/// untouched — so, unlike `update`, it never calls for a reconnect.
pub fn set_avatar(
    conn: &Connection,
    id: &str,
    avatar_path: Option<&str>,
) -> rusqlite::Result<Option<Character>> {
    let now = Utc::now().to_rfc3339();
    let affected = conn.execute(
        "UPDATE characters SET avatar_path = ?1, updated_at = ?2 WHERE id = ?3",
        params![avatar_path, now, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM characters WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_characters_count_double() {
        assert_eq!(name_width("Emma"), 4);
        assert_eq!(name_width("小柔"), 4);
        assert_eq!(name_width("小柔 Emma"), 9);
        assert_eq!(name_width("ＡＢ"), 4, "full-width Latin");
        assert_eq!(name_width("한국"), 4);
        assert_eq!(name_width("😀"), 2);
    }

    #[test]
    fn names_are_trimmed_and_capped() {
        assert_eq!(check_name("  小柔 ").as_deref(), Ok("小柔"));
        assert!(check_name("   ").is_err());

        assert!(check_name(&"字".repeat(12)).is_ok());
        assert!(check_name(&"字".repeat(13)).is_err());
        assert!(check_name(&"a".repeat(24)).is_ok());
        assert!(check_name(&"a".repeat(25)).is_err());
    }
}
