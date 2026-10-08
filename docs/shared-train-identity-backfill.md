# Deploying the shared-train-identity change to a database with existing data

`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md` is an
expand/contract migration. Its final, **irreversible** step
(`crates/ds-store/migrations/20260906140000_drop_legacy_columns.sql`) drops
`train_movement_events.tracked_train_id`,
`train_current_state.tracked_train_id`, and seven legacy columns from
`tracked_trains` — including `train_uid`. Those are the only columns from
which a pre-existing row's shared-train identity can still be recovered, so
they must not be dropped until every such row has been re-pointed at a
`trains` row via its new `trains_id` column.

A **fresh, empty database is unaffected**: there are no pre-existing rows,
every migration applies in order, and nothing below is needed. So is any
database that has already applied `20260906140000`.

## Required sequence (database with pre-existing data)

1. **Deploy the expand phase only.** Any build whose migrations stop before
   `20260906140000` — or simply let the current `api` start once; see step 4
   for why it will stop itself if you skip ahead.
2. **Run the backfill.** It is idempotent, safe to run repeatedly, and safe
   to re-run after a partial failure.

   ```sh
   # From a checkout, as the schema owner (DATABASE_URL is the fallback)
   MIGRATION_DATABASE_URL=postgres://owner@... cargo run -p ds-migrate -- backfill-trains

   # From the api container image (ds-migrate ships alongside `api`)
   /usr/local/bin/ds-migrate backfill-trains
   ```

   It refuses the api's own role (`distant_signal_api`): since the ingest
   phase 5 prep (Q2 of `docs/ingest-phase5-runbook.md`) these one-offs never
   run with the api's credentials. The old `backfill_trains` binary in the
   api image still works but is deprecated; it goes in phase 5 step 5.4b.

3. **Check the output.** It reports how many subscription /
   `train_movement_events` / `train_current_state` rows it linked, plus
   `remaining gaps`. Remaining gaps are rows that carry no legacy identity
   at all (a subscription that never resolved, or a movement row beneath
   one) — the design's own accepted §1/§2-Step-B gap. There is nothing for
   the drop to lose in those rows; a non-zero count is not a blocker.
4. **Deploy the build containing `20260906140000`.**

## This ordering is enforced, not just documented

`api`'s startup calls
`ds_store::migrate::ensure_ready_for_contract_migration` immediately
before `sqlx::migrate!().run(...)`. If `20260906140000` has not been applied
yet and rows still exist that a backfill *would* have linked, `api` refuses
to start and names this document's step 2 in the error. It cannot silently
drop recoverable data.

The check lives at startup rather than inside the migration file because the
migration has already been applied in every existing environment: sqlx
validates the checksum of every applied migration on connect, so editing
that file would break `sqlx migrate run` on exactly the databases the check
was meant to protect — and would never re-run there anyway. A migration
*added* after it cannot gate it either, since migrations apply in version
order.

Implementation and reasoning: `crates/api/src/data/legacy_backfill.rs`.
