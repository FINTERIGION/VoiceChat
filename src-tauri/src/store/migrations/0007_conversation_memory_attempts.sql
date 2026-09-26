-- How many times a queued conversation's summary has failed. A failed one
-- stays at the head of its character's memory queue, holding back everything
-- after it so nothing lands in the rolling summary ahead of it (see
-- `realtime::session::drain_memory_queue`); this is what lets the queue give
-- up on it at `MAX_MEMORY_ATTEMPTS`, so one the API always rejects can't hold
-- the character's memory back for good.
ALTER TABLE conversations ADD COLUMN memory_attempts INTEGER NOT NULL DEFAULT 0;
