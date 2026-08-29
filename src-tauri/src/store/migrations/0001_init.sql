CREATE TABLE characters (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    avatar_path TEXT,
    language TEXT NOT NULL DEFAULT 'auto', -- zh|ja|en|auto
    persona TEXT NOT NULL DEFAULT '',
    speech_habits TEXT NOT NULL DEFAULT '',
    greeting TEXT NOT NULL DEFAULT '',
    voice_kind TEXT NOT NULL DEFAULT 'preset', -- preset|designed|cloned
    voice_id TEXT,
    voice_prompt TEXT,
    memory_enabled INTEGER NOT NULL DEFAULT 1,
    turn_detection_mode TEXT NOT NULL DEFAULT 'ptt', -- ptt|vad
    vad_threshold REAL NOT NULL DEFAULT 0.5,
    silence_ms INTEGER NOT NULL DEFAULT 800,
    max_history_turns INTEGER NOT NULL DEFAULT 20,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    character_id TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT
);

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL, -- user|assistant
    text TEXT NOT NULL DEFAULT '',
    audio_ms INTEGER,
    created_at TEXT NOT NULL
);

CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    character_id TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    kind TEXT NOT NULL, -- profile|fact|summary
    content TEXT NOT NULL,
    salience REAL NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX idx_conversations_character ON conversations(character_id);
CREATE INDEX idx_messages_conversation ON messages(conversation_id);
CREATE INDEX idx_memories_character ON memories(character_id);
