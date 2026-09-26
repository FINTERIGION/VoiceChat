-- The audio each custom voice was cloned from, kept so that a character using
-- it can be shared without asking the user to supply the audio again: a voice
-- id means nothing in someone else's DashScope account, but its sample can be
-- cloned there (see `store::share`).
--
-- Keyed by voice rather than hung off a character, because a sample belongs
-- to a voice: several characters can use one, and a voice picked again from
-- the Voice Studio's cloud tab still has it. `file_name` names a file in the
-- app data directory's `voice_samples/` folder (see `voice::sample`).
CREATE TABLE voice_samples (
    voice_id   TEXT PRIMARY KEY,
    file_name  TEXT NOT NULL,
    created_at TEXT NOT NULL
);
