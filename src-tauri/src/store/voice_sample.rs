//! Which stored sample each custom voice was cloned from. The files
//! themselves live in `voice_samples/`; see `voice::sample`.

use std::collections::HashSet;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

/// The file holding `voice_id`'s sample, if one was kept.
pub fn get(conn: &Connection, voice_id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT file_name FROM voice_samples WHERE voice_id = ?1",
        params![voice_id],
        |row| row.get(0),
    )
    .optional()
}

/// Records `file_name` as `voice_id`'s sample, returning the file it
/// replaces — which nothing points at any more — if there was one.
pub fn set(conn: &Connection, voice_id: &str, file_name: &str) -> rusqlite::Result<Option<String>> {
    let replaced = get(conn, voice_id)?.filter(|old| old != file_name);
    conn.execute(
        "INSERT INTO voice_samples (voice_id, file_name, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(voice_id) DO UPDATE SET
            file_name = excluded.file_name,
            created_at = excluded.created_at",
        params![voice_id, file_name, Utc::now().to_rfc3339()],
    )?;
    Ok(replaced)
}

/// Forgets `voice_id`'s sample, returning the file that held it.
pub fn delete(conn: &Connection, voice_id: &str) -> rusqlite::Result<Option<String>> {
    let file = get(conn, voice_id)?;
    conn.execute(
        "DELETE FROM voice_samples WHERE voice_id = ?1",
        params![voice_id],
    )?;
    Ok(file)
}

/// Every file a voice points at, for `voice::sample::sweep` to keep.
pub fn file_names(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT file_name FROM voice_samples")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db;

    struct TempDb {
        path: std::path::PathBuf,
        conn: Connection,
    }

    impl TempDb {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("voicechat-voice-sample-test-{}.db", uuid::Uuid::new_v4()));
            let conn = db::open(&path).expect("open");
            Self { path, conn }
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn replacing_or_deleting_hands_back_the_file_let_go_of() {
        let db = TempDb::new();
        assert_eq!(set(&db.conn, "v1", "a.wav").expect("set"), None);
        assert_eq!(get(&db.conn, "v1").expect("get").as_deref(), Some("a.wav"));

        assert_eq!(
            set(&db.conn, "v1", "b.wav").expect("replace").as_deref(),
            Some("a.wav")
        );
        assert_eq!(
            set(&db.conn, "v1", "b.wav").expect("same again"),
            None,
            "the file still in use isn't handed back for removal"
        );
        set(&db.conn, "v2", "c.wav").expect("set");
        assert_eq!(
            file_names(&db.conn).expect("names"),
            HashSet::from(["b.wav".to_string(), "c.wav".to_string()])
        );

        assert_eq!(delete(&db.conn, "v1").expect("delete").as_deref(), Some("b.wav"));
        assert_eq!(get(&db.conn, "v1").expect("get"), None);
        assert_eq!(delete(&db.conn, "v1").expect("delete again"), None);
    }
}
