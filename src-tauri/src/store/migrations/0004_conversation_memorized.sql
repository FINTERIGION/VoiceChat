-- Whether a conversation has been summarized into its character's long-term
-- memory — which decides, for one, whether deleting it from the history
-- leaves anything of it behind.
--
-- Every conversation is kept in the history now, including ones the
-- character has memory switched off for and ones the user opted out of with
-- "本次不计入记忆", so being stored no longer implies being remembered. Set
-- only once a summary has actually landed in `memories` (see
-- `realtime::session::summarize_into_memory`), never just because the
-- conversation was meant to count.
ALTER TABLE conversations ADD COLUMN memorized INTEGER NOT NULL DEFAULT 0;

-- Until now a conversation was only stored while it was being recorded into
-- memory, and every one that ended was summarized on the way out. The
-- exception is a conversation the app never got to end — killed, crashed —
-- which `close_dangling_conversations` closed on the next launch without
-- summarizing, stamping `ended_at` with its last message's time. A real end
-- is stamped after that message (and after the naming call), so the two can
-- be told apart. One whose summary call failed can't be, and is marked
-- anyway.
UPDATE conversations SET memorized = 1
 WHERE ended_at IS NOT NULL
   AND ended_at <> (SELECT MAX(created_at) FROM messages
                     WHERE conversation_id = conversations.id);
