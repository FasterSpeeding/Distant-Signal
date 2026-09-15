-- -------------------------------------------------------------------------
-- Normalize blank users.name/users.email to NULL.
--
-- An identity provider with no name on file for a user does not omit the
-- `name` claim -- Authentik's stock `profile` scope mapping (the one this
-- deployment attaches) returns its `User.name` attribute verbatim, and
-- that attribute defaults to the empty string. Until now
-- data::users::upsert_user wrote that claim through unchanged, so
-- `users.name` holds `''` for every such user.
--
-- Everything that reads these columns treats "no name" as NULL and falls
-- back accordingly -- `''` is not NULL, so it won the fallback and was
-- rendered as the label itself: a group member row that was just a role
-- badge, a "Shared by " with nothing after it, and an empty nav-bar label.
--
-- `upsert_user` now normalizes on write, but that only takes effect on a
-- user's NEXT login; this backfills the rows already stored. btrim as well
-- as NULLIF, matching `data::users::non_blank`'s own trim -- a name of
-- "  Ada  " should never be rendered with its padding intact either.
--
-- Idempotent, and a no-op on a database where neither has ever happened.
-- -------------------------------------------------------------------------

UPDATE users
   SET name = NULLIF(btrim(name), '')
 WHERE name IS NOT NULL
   AND name IS DISTINCT FROM NULLIF(btrim(name), '');

UPDATE users
   SET email = NULLIF(btrim(email), '')
 WHERE email IS NOT NULL
   AND email IS DISTINCT FROM NULLIF(btrim(email), '');
