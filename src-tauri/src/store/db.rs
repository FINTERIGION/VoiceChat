use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_init.sql"),
    include_str!("migrations/0002_drop_unused_character_columns.sql"),
    include_str!("migrations/0003_conversation_titles.sql"),
    include_str!("migrations/0004_conversation_memorized.sql"),
];

pub fn open(db_path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        // The migration and the version bump recording it commit together.
        // Bumping separately leaves a window where a crash (or a failure
        // part-way through a multi-statement migration) applies the work
        // without recording it, so the next launch replays statements that
        // already ran. For a migration like 0002 that means erroring on a
        // column it just dropped — which fails `open` and wedges the app on
        // every subsequent start, with no way out but deleting the database.
        conn.execute_batch(&format!(
            "BEGIN;\n{sql}\nPRAGMA user_version = {};\nCOMMIT;",
            i + 1
        ))?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Migration 0004 marks the conversations that ended properly — and so
    /// were summarized on the way out — and leaves alone the ones a crash
    /// left for the dangling-row sweep, or that are still open.
    #[test]
    fn backfills_the_memory_mark_for_properly_ended_conversations() {
        let conn = Connection::open_in_memory().expect("open");
        // A database as the build before 0004 left it.
        for (i, sql) in MIGRATIONS[..3].iter().enumerate() {
            conn.execute_batch(&format!("{sql}\nPRAGMA user_version = {};", i + 1))
                .expect("earlier migrations");
        }
        conn.execute_batch(
            "INSERT INTO characters (id, name, created_at, updated_at) VALUES ('c', 'Nia', 't', 't');
             INSERT INTO conversations (id, character_id, started_at, ended_at) VALUES
                 ('ended', 'c', '2026-01-01T10:00:00+00:00', '2026-01-01T10:05:00+00:00'),
                 ('swept', 'c', '2026-01-01T11:00:00+00:00', '2026-01-01T11:01:00+00:00'),
                 ('open',  'c', '2026-01-01T12:00:00+00:00', NULL);
             INSERT INTO messages (id, conversation_id, role, text, created_at) VALUES
                 ('m1', 'ended', 'user', 'hi', '2026-01-01T10:01:00+00:00'),
                 ('m2', 'swept', 'user', 'hi', '2026-01-01T11:01:00+00:00'),
                 ('m3', 'open',  'user', 'hi', '2026-01-01T12:01:00+00:00');",
        )
        .expect("seed");

        migrate(&conn).expect("migrate");

        let memorized = |id: &str| -> bool {
            conn.query_row(
                "SELECT memorized FROM conversations WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("row")
        };
        assert!(memorized("ended"));
        assert!(!memorized("swept"));
        assert!(!memorized("open"));
    }
}
