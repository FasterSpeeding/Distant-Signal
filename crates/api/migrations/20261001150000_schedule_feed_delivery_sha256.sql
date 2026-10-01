SET LOCAL lock_timeout = '5s';

-- -------------------------------------------------------------------------
-- Provenance of each delivered file (docs/schedule-feed-sftp.md, Ranma's
-- sftp-audit-observability spec): schedule-ingest now hashes every file the
-- SFTP push delivers, so "which bytes did we accept" can be answered later
-- and joined to SFTPGo's Upload log line by name, size and time.
--
-- schedule_feed_ingests: the delivered zip itself. The extracted entries'
-- hashes go into each element of the existing `files` JSONB array
-- ({name, bytes, sha256}), which needs no DDL.
-- corpus_deliveries: the CORPUS file (its name is already source_file).
--
-- All nullable: rows from before this migration have no hash, and an older
-- schedule-ingest still posts without one. The CHECKs keep whatever is
-- stored a lowercase hex SHA-256. Both tables hold one row per delivery
-- (a few hundred rows), so validating the constraints is instant.
-- -------------------------------------------------------------------------

ALTER TABLE schedule_feed_ingests
    ADD COLUMN source_file TEXT,
    ADD COLUMN source_bytes BIGINT CHECK (source_bytes >= 0),
    ADD COLUMN source_sha256 TEXT CHECK (source_sha256 ~ '^[0-9a-f]{64}$');

ALTER TABLE corpus_deliveries
    ADD COLUMN source_bytes BIGINT CHECK (source_bytes >= 0),
    ADD COLUMN sha256 TEXT CHECK (sha256 ~ '^[0-9a-f]{64}$');
