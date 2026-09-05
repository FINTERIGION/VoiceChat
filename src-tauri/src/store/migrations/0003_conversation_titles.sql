-- Names a conversation so the Chat tab's history list can show something
-- other than a timestamp.
--
-- NULL means "not named yet": every conversation starts that way and gets
-- filled in by the LLM once there is a turn to name it after (see
-- `realtime::session::name_conversation`), or by the user renaming it by
-- hand. There is deliberately no flag distinguishing the two — a name that
-- exists is never regenerated, so a manual rename always sticks.
ALTER TABLE conversations ADD COLUMN title TEXT;
