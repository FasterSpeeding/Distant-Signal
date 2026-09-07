# Deploying the shared-train-identity change to a database with existing data

`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md` is an
expand/contract migration. Its final, **irreversible** step
(`crates/api/migrations/20260906140000_drop_legacy_columns.sql`) drops
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
   # From a checkout
   DATABASE_URL=postgres://... cargo run -p api --bin backfill_trains

   # From the api container image (the binary ships alongside `api` itself)
   /usr/local/bin/backfill_trains
   ```

3. **Check the output.** It reports how many subscription /
   `train_movement_events` / `train_current_state` rows it linked, plus
   `remaining gaps`. Remaining gaps are rows that carry no legacy identity
   at all (a subscription that never resolved, or a movement row beneath
   one) — the design's own accepted §1/§2-Step-B gap. There is nothing for
   the drop to lose in those rows; a non-zero count is not a blocker.
4. **Deploy the build containing `20260906140000`.**

## This ordering is enforced, not just documented

`api`'s startup calls
`data::legacy_backfill::ensure_ready_for_contract_migration` immediately
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
