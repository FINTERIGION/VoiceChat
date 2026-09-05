use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

pub struct Conversation {
    pub id: String,
}

/// One row of the Chat tab's history list.
///
/// `title` is `None` until the conversation has been named; `preview` (its
/// opening line) stands in for it until then, so a row is never blank.
#[derive(Debug, Clone, Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub character_id: String,
    pub title: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub message_count: i64,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub created_at: String,
}

/// The two correlated subqueries are what make a summary row: how much was
/// said, and the opening line to label it with while it has no title. Empty
/// texts are skipped for the preview — a user turn whose transcription never
/// arrived is stored blank, and labelling a whole conversation with it would
/// leave the row looking empty.
const SUMMARY_SELECT: &str = "SELECT c.id, c.character_id, c.title, c.started_at, c.ended_at, \
     (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) AS message_count, \
     COALESCE((SELECT text FROM messages WHERE conversation_id = c.id AND text <> '' \
               ORDER BY created_at ASC LIMIT 1), '') AS preview \
     FROM conversations c";

fn row_to_summary(row: &rusqlite::Row) -> rusqlite::Result<ConversationSummary> {
    Ok(ConversationSummary {
        id: row.get("id")?,
        character_id: row.get("character_id")?,
        title: row.get("title")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        message_count: row.get("message_count")?,
        preview: row.get("preview")?,
    })
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

/// Newest first, and only conversations that hold something.
///
/// Every connect opens a row, so without the `EXISTS` filter the history
/// list would fill up with sessions nobody spoke in — an idle timeout, a
/// character switched away from before saying anything, a reconnect after a
/// dropped socket.
pub fn list_conversations(
    conn: &Connection,
    character_id: &str,
) -> rusqlite::Result<Vec<ConversationSummary>> {
    let mut stmt = conn.prepare(&format!(
        "{SUMMARY_SELECT} WHERE c.character_id = ?1 \
         AND EXISTS (SELECT 1 FROM messages WHERE conversation_id = c.id) \
         ORDER BY c.started_at DESC"
    ))?;
    let rows = stmt.query_map(params![character_id], row_to_summary)?;
    rows.collect()
}

pub fn get_conversation(
    conn: &Connection,
    id: &str,
) -> rusqlite::Result<Option<ConversationSummary>> {
    conn.query_row(
        &format!("{SUMMARY_SELECT} WHERE c.id = ?1"),
        params![id],
        row_to_summary,
    )
    .optional()
}

/// Whether this conversation still needs a name. False for one already
/// named — including by hand — and for one that no longer exists.
///
/// Only an optimization: it lets the automatic naming pass skip the LLM
/// call it was about to make. What actually keeps that pass from
/// overwriting a name is `set_title_if_untitled`.
pub fn is_untitled(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let title: Option<Option<String>> = conn
        .query_row(
            "SELECT title FROM conversations WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(matches!(title, Some(t) if t.as_deref().unwrap_or("").trim().is_empty()))
}

/// Renames unconditionally — what the user typed always wins.
pub fn set_title(
    conn: &Connection,
    id: &str,
    title: &str,
) -> rusqlite::Result<Option<ConversationSummary>> {
    let changed = conn.execute(
        "UPDATE conversations SET title = ?1 WHERE id = ?2",
        params![title, id],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    get_conversation(conn, id)
}

/// Names a conversation only if it still has no name, and reports whether
/// it wrote one.
///
/// For the automatic naming pass, whose title is a second or so of LLM call
/// out of date by the time it lands: in that window the user may have
/// renamed the conversation by hand, or a second naming attempt may have
/// finished first. The `WHERE` clause makes losing that race a no-op
/// instead of an overwrite.
pub fn set_title_if_untitled(conn: &Connection, id: &str, title: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "UPDATE conversations SET title = ?1 \
         WHERE id = ?2 AND (title IS NULL OR trim(title) = '')",
        params![title, id],
    )?;
    Ok(changed > 0)
}

/// Closes conversations a previous run left open — the app killed, the
/// machine shut down, or a crash between the last message and the end of the
/// session. Returns how many rows it fixed.
///
/// Only safe to call before the session actor exists, since it cannot tell a
/// stale row from one being written to right now.
///
/// `ended_at` is stamped with the conversation's last message rather than
/// "now": it stopped when the app did, and dating it to the next launch —
/// possibly days later — would put a lie in the history list. One that never
/// held a message falls back to when it started.
pub fn close_dangling_conversations(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE conversations SET ended_at = COALESCE( \
             (SELECT MAX(created_at) FROM messages WHERE conversation_id = conversations.id), \
             started_at) \
         WHERE ended_at IS NULL",
        [],
    )
}

/// The `messages` rows go with it, via `ON DELETE CASCADE`.
pub fn delete_conversation(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
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
        "SELECT id, role, text, created_at FROM messages \
         WHERE conversation_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok(Message {
            id: row.get("id")?,
            role: row.get("role")?,
            text: row.get("text")?,
            created_at: row.get("created_at")?,
        })
    })?;
    rows.collect()
}
