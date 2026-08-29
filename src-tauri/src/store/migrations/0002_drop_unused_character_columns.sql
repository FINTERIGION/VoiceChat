-- Removes four columns from `characters` that no longer back any feature.
--
-- `greeting` was editable in the character form and written to the database,
-- but nothing ever read it back: it never reached the model's instructions,
-- so the opening line a user typed had no effect. The field and its UI were
-- removed rather than wired up.
--
-- `turn_detection_mode`, `vad_threshold` and `silence_ms` date from when
-- turn-taking was configured per character. Hands-free detection is now the
-- only mode, and its two knobs are a single global preference (`vad_threshold`
-- and `vad_silence_ms` in the `settings` table) because how sensitive
-- detection should be depends on the microphone and the room, not on which
-- character you are talking to.
--
-- DROP COLUMN needs SQLite 3.35+; the bundled build is 3.53. None of these
-- columns are indexed or referenced by a constraint, view or trigger, so each
-- drop is a plain metadata edit.
ALTER TABLE characters DROP COLUMN greeting;
ALTER TABLE characters DROP COLUMN turn_detection_mode;
ALTER TABLE characters DROP COLUMN vad_threshold;
ALTER TABLE characters DROP COLUMN silence_ms;
