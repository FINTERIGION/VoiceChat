use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

pub struct Conversation {
    pub id: String,
}

pub struct Message {
    pub role: String,
    pub text: String,
}

pub fn start_conversation(conn: &Connection, character_id: &str) -> rusqlite::Result<Conversation> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO conversations (id, character_id, started_at, ended_at) VALUES (?1, ?2, ?3, NULL)",
        params![id, character_id, now],
    )?;
    Ok(Conversation { id })
}

pub fn end_conversation(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE conversations SET ended_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn insert_message(
    conn: &Connection,
    conversation_id: &str,
    role: &str,
    text: &str,
) -> rusqlite::Result<()> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, text, audio_ms, created_at) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
        params![id, conversation_id, role, text, now],
    )?;
    Ok(())
}

pub fn list_messages(conn: &Connection, conversation_id: &str) -> rusqlite::Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT role, text FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok(Message {
            role: row.get(0)?,
            text: row.get(1)?,
        })
    })?;
    rows.collect()
}
