-- Which catalogue lines each incident matches, as decided by the ONE
-- matcher this codebase has (`common::matcher::lines_affected_by`, the same
-- function the aggregator runs to build live line status).
--
-- Why a new column rather than finally populating `affected_stations`:
-- RDM's Knowledgebase Incidents XML has no CRS field at all -- only a
-- free-text `Affects.RoutesAffected` -- so `affected_stations` has been
-- `'{}'` on every production row since the table was created, and the
-- archive's Line filter (`affected_stations && $2`) consequently returned
-- zero rows for every line including National Rail. See
-- docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md 1c.
-- Line-level attribution is the answer the matcher already produces from
-- prose keywords + the feed's structured operator list, and is what the
-- Line filter actually wants; deriving CRS codes from prose to then map
-- them back to lines would be a strictly lossier route to the same answer.
--
-- Additive and defaulted, so existing rows are valid immediately (they
-- simply match no line until backfilled -- exactly today's behaviour, not a
-- regression). `crates/api/src/bin/backfill_incident_lines.rs` fills them
-- in; see docs/incident-affected-lines-backfill.md.
ALTER TABLE incidents
    ADD COLUMN affected_lines TEXT[] NOT NULL DEFAULT '{}';

-- Mirrors incidents_operators_gin: the Line filter is an array-overlap
-- predicate, which is exactly what GIN on a text[] serves.
CREATE INDEX incidents_affected_lines_gin ON incidents USING GIN (affected_lines);
