# Correction: the real CIF SCHEDULE feed delivery is a single zip, not a manifest + flat files

**Status: correction, not a new design.** Records what
`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` (the push
design doc) and
`docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md`
got wrong about the real delivery's *outer* shape, and points at the fix.
Following this repo's established "Corrections" convention (see e.g.
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md`'s
"Corrections to the brief's assumptions" section) rather than rewriting
either design doc in place.

**Source: the repo owner, confirmed directly, 2026-09-03.** Not
independently re-discovered via `WebFetch`/`WebSearch` in this pass —
flagged per this app's research documents' established convention of
attributing task-given claims to their source.

## What was assumed

Both design docs, and `crates/schedule-ingest/src/manifest.rs`'s own module
doc comment (which went as far as claiming the manifest format was
"confirmed directly against a sample delivery"), assumed the real delivery
follows RSPS5046's documented SFTP-push manifest model: a text manifest
(`RJTTFnnnDAT.txt`) listing the names of ~8 sibling flat CIF files
(`RJTTFnnnMCA.txt`, `MSN`, `ZTR`, `REJ`, `SET`, `FLF`, `ALF`, `TSI`) that
land side-by-side in `watch_dir`, verified complete by cross-checking the
manifest's file list, with a `Sequence:` number used to detect gaps/dedup
between deliveries (`manifest.rs`'s `SequenceRelation`/`classify_sequence`).

## What's actually true

The real "raildata push API" delivers a **single `.zip` archive** via the
already-configured SFTPGo push mechanism, landing in `watch_dir`
(`/data/schedule-feed/incoming/`), **overwritten in place** on every new
delivery (same path, new content/mtime each time). There is:

- **No manifest.** Nothing lists or cross-checks the archive's contents.
- **No sequence number** exposed at the delivery level to compare between
  deliveries. (A `942`-style number is still embedded in the filenames
  *inside* the zip, e.g. `RJTTF942MCA.txt` — but it is not a reliable
  cross-delivery identifier either; see below.)
- **No multi-file delivery.** One file, one event.

What *was* right: the zip's internal contents match exactly what this
pipeline was always designed to consume — the same `RJTTFnnn*.txt`-named
CIF files, independently byte-inspected against a real sample
(`timetable_full.zip`) during this pipeline's original research. It's
specifically the *outer* delivery shape (one zip, no manifest, no sequence)
that both design docs got wrong, not the CIF file format inside it.

The embedded `942`-style number inside a real delivery's own filenames is
also not safe to lean on as a stable identifier for "which delivery is
this" going forward — it is not the same kind of number as the outer
delivery's own arrival time, and nothing guarantees it stays contiguous or
even present in exactly that shape. `schedule-reference`'s discovery logic
was rewritten to never reconstruct filenames from it or use it to decide
recency (see the fix, below); it survives in one place only as an
opportunistic, non-authoritative provenance value
(`common::StanoxCrsRecord::source_sequence`, a pre-existing field shared
with `trust-consumer` that was out of this fix's scope to retype).

## The fix

`crates/schedule-ingest`, `crates/schedule-reference`, and the
`schedule_feed_ingests` table/route/query in `crates/api` were reworked
around the real single-zip, no-manifest, no-sequence delivery shape:

- **Detection**: match any `*.zip` file present in `watch_dir` (not the
  literal `timetable_full.zip`), most-recently-modified wins if more than
  one is ever present at once (`crates/schedule-ingest/src/delivery.rs`).
- **Stability**: unchanged — `scan.rs`'s `StabilityTracker`/`scan_incoming`
  is reused as-is, just pointed at the zip candidate.
- **Dedup**: mtime-based (`delivery::classify_delivery`), replacing
  `manifest.rs`'s sequence-based `classify_sequence` entirely. There is no
  equivalent of the old `Gap` variant — without a sequence number there is
  nothing to be non-contiguous.
- **Storage**: each stable delivery is extracted (not moved as a zip) into
  `storage_dir/<YYYYMMDDTHHMMSSZ>/`, a compact UTC rendering of the
  delivery's own mtime that sorts lexicographically == chronologically —
  `prune_old_sequences` (renamed `prune_old_deliveries`) and
  `schedule-reference`'s discovery logic (`sequence.rs`, renamed
  `discovery.rs`) both rely on that ordering.
- **`schedule_feed_ingests`**: rekeyed on `delivered_at TIMESTAMPTZ PRIMARY
  KEY` (the delivery zip's own mtime) instead of `sequence INTEGER PRIMARY
  KEY` — see migration `20260903090000_schedule_feed_ingests_delivered_at.sql`.
  `/public/freshness`'s `schedule_feed` field now reflects
  `MAX(delivered_at)` rather than `MAX(ingested_at)`, which is if anything a
  *more* meaningful signal than before (previously it was `Utc::now()` at
  whatever moment `schedule-ingest` happened to process something, not tied
  to when DTD actually delivered anything).

No history/retention requirement exists today beyond "keep the N most
recent deliveries on disk" (unchanged in spirit from before, just renamed).
If a future need arises to keep additional historical copies long-term, the
repo owner's stated preference is a separately-callable copy step to a
distinct location — deliberately not built as part of this fix, but nothing
here makes it harder to add later.
