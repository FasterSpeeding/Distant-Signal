-- -------------------------------------------------------------------------
-- chatbot_allowed_users: the DS-hosted chat orchestrator (Option B)'s cost/
-- access gate. See
-- docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md's Task 2
-- and docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md's
-- Decision 5.
--
-- A bare allowlist -- membership alone, no per-row metadata -- since
-- nothing today needs more than "may this user reach the chatbot at all."
-- `ON DELETE CASCADE`, matching this schema's established posture for a
-- row that only ever means something in relation to a `users` row (the
-- same reasoning custom_lines/tracked_trains' own ownership columns
-- already use): an allowlist entry for a deleted user is meaningless, not
-- an orphan worth preserving.
-- -------------------------------------------------------------------------

CREATE TABLE chatbot_allowed_users (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE
);
