use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Memory {
    pub id: String,
    pub character_id: String,
    /// profile|fact|summary
    pub kind: String,
    pub content: String,
    pub salience: f64,
    pub updated_at: String,
}

fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get("id")?,
        character_id: row.get("character_id")?,
        kind: row.get("kind")?,
        content: row.get("content")?,
        salience: row.get("salience")?,
        updated_at: row.get("updated_at")?,
    })
}

const COLUMNS: &str = "id, character_id, kind, content, salience, updated_at";

pub fn list(conn: &Connection, character_id: &str) -> rusqlite::Result<Vec<Memory>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM memories WHERE character_id = ?1 ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map(params![character_id], row_to_memory)?;
    rows.collect()
}

/// Highest-salience memories first (ties broken by recency), for injection
/// into `instructions`.
pub fn top_k(conn: &Connection, character_id: &str, k: usize) -> rusqlite::Result<Vec<Memory>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM memories WHERE character_id = ?1 \
         ORDER BY salience DESC, updated_at DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![character_id, k as i64], row_to_memory)?;
    rows.collect()
}

pub fn create(
    conn: &Connection,
    character_id: &str,
    kind: &str,
    content: &str,
    salience: f64,
) -> rusqlite::Result<Memory> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO memories (id, character_id, kind, content, salience, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, character_id, kind, content, salience, now],
    )?;
    Ok(Memory {
        id,
        character_id: character_id.to_string(),
        kind: kind.to_string(),
        content: content.to_string(),
        salience,
        updated_at: now,
    })
}

pub fn update_content(conn: &Connection, id: &str, content: &str) -> rusqlite::Result<Option<Memory>> {
    let now = Utc::now().to_rfc3339();
    let affected = conn.execute(
        "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
        params![content, now, id],
    )?;
    if affected == 0 {
        return Ok(None);
    }
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM memories WHERE id = ?1"),
        params![id],
        row_to_memory,
    )
    .optional()
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn delete_all_for_character(conn: &Connection, character_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM memories WHERE character_id = ?1",
        params![character_id],
    )?;
    Ok(())
}

/// The rolling summary is a single row per character: replace, don't
/// accumulate.
pub fn replace_summary(conn: &Connection, character_id: &str, content: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM memories WHERE character_id = ?1 AND kind = 'summary'",
        params![character_id],
    )?;
    if !content.trim().is_empty() {
        create(conn, character_id, "summary", content, 1.0)?;
    }
    Ok(())
}

/// Keeps unbounded fact accumulation in check: drop the least salient/oldest
/// facts once there are more than `max`.
pub fn cap_facts(conn: &Connection, character_id: &str, max: usize) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM memories WHERE id IN (
            SELECT id FROM memories
            WHERE character_id = ?1 AND kind = 'fact'
            ORDER BY salience DESC, updated_at DESC
            LIMIT -1 OFFSET ?2
        )",
        params![character_id, max as i64],
    )?;
    Ok(())
}
