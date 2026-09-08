-- New index required by the tracked-train resolution reconciliation
-- sweep's schedule-enrichment retry
-- (docs/superpowers/specs/2026-09-08-tracked-train-resolution-reconciliation-design.md
-- §3): neither existing index on schedule_destination_departures leads
-- with train_uid -- the primary key leads with (service_date,
-- destination_crs, ...) and the calling-point-search index leads with
-- (service_date, origin_crs, ...) -- so a (service_date, train_uid)
-- point lookup would otherwise force a filtered scan of up to one whole
-- rail day's ~377k rows per attempt.
--
-- This is the train_uid-keyed reverse lookup
-- 2026-09-06-shared-train-identity-design.md §1 named as an accepted,
-- deferred gap ("This design does not build a UID-keyed reverse schedule
-- index... deferred to whenever the search sub-project is actually
-- scoped"). The search sub-project has since shipped
-- schedule_destination_departures itself; this is that index, added now
-- that a concrete, justified caller exists.
CREATE INDEX schedule_destination_departures_train_uid_idx
    ON schedule_destination_departures (service_date, train_uid);
