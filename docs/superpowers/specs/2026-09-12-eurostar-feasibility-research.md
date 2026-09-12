# Research: Is Eurostar Coverage Feasible with Distant Signal's Existing Data Access?

**Status: researched and closed. Conclusion: not feasible with this app's current CIF/TRUST data access — full stop, not a subscription-tier or code-level fix.**

Three rounds of investigation, summarized here in order. No code was written; this is a pure research finding, kept for anyone who reconsiders this idea later so the same ground doesn't need covering twice.

## Round 1: initial feasibility research (codebase + general knowledge)

Started from a corrected premise: Eurostar's UK-territory running (London St Pancras through the Channel Tunnel portal, i.e. Dollands Moor/Cheriton, plus Temple Mills International Depot empty-stock moves) genuinely operates over National Rail infrastructure — it is not purely a foreign/international-only operation once inside GB. On that basis:

- Eurostar has a real ATOC/TOC code, **`ES`**, and real British Train Reporting Numbers (headcodes): `9Ixx` for Brussels services, `9Oxx` for Paris/other French destinations, `9Sxx` for empty-stock moves to/from Temple Mills. These only exist because a schedule was planned through the industry's standard process — the same process that produces CIF `BS` records.
- This app's own ingestion code (`crates/schedule-ingest`, `crates/schedule-reference`, `crates/trust-consumer`, `crates/trust-schema`) has **no TOC allowlist or filter anywhere** — confirmed by grep. These pipelines are operator-agnostic; selection happens downstream (`lines/*.toml`'s `operators` field, or the train-search/tracking routes), not at ingest. So in principle nothing in the code itself excludes Eurostar.
- **LDBWS/Darwin confirmed absent, structurally.** St Pancras has two separate station codes: `STP` (domestic, Darwin/LDBWS-covered) and `SPX` (Eurostar's gated international side). Multiple sources agree `SPX` is not part of the Darwin/National Rail live-departures feed — matches this app's own existing `docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md:61` ("Eurostar is out of scope (international, not National Rail)").
- **Recommended shape, if it ever became feasible**: not a separate subsystem (unlike Island of Ireland) — Eurostar's usable data would arrive through the *same* National-Rail-shaped CIF/TRUST ecosystem this app already ingests. Not a `lines/*.toml` entry either — that schema exists to arbitrate incidents across multiple operators sharing track; Eurostar is a single operator with no severity-inference input of its own. Best fit: a small additive change to the existing individual-train-tracking feature (search/track one Eurostar service by headcode/UID for its UK leg), not a line-status feature.
- **One open question flagged**: whether Eurostar's `ES`/`9`-series records are actually present in this app's specific Rail Data Marketplace subscription — a question only a live-feed check could close.

## Round 2: first live production-data check

Direct, read-only queries against the live database (not general reasoning):

- `stanox_crs` (this app's own real STANOX↔CRS mapping, built entirely from CIF/TRUST data it has actually processed): **zero entries** for `SPX`, `EBD` (Ebbsfleet International), or `AFK` (Ashford International) — despite all three existing as real stations in the broader `stations` reference table (a different, more general RDM feed).
- `schedule_destination_departures` (built straight from real CIF ingest, no translation step): **zero rows** with any of those 3 CRS codes as origin or destination.
- `trains.train_uid LIKE 'ES%'`: **zero** rows.
- A loose `trust_event_backlog.train_id LIKE '%9I%'/'%9O%'/'%9S%'` search initially looked promising but was **100% false positives** — real headcodes belonging to ordinary Thameslink/Great Northern suburban services (Farringdon, Gatwick, Hitchin) that merely contain those 2-character substrings mid-string.

Conclusion at this point: no evidence of Eurostar in this app's actual ingested data, contradicting Round 1's hopeful theoretical case — but not yet tested as rigorously as possible (no external ground truth, `STP` itself not checked, `SFA`/Stratford International not checked).

## Round 3: deeper, falsifiable follow-up

This round fixed every gap Round 2 left open:

**A real external test case.** Fetched Eurostar's own live timetable (London St Pancras → Paris Gare du Nord, 2026-09-12): 15 real scheduled departures, e.g. `ES 9002` dep STP 06:31, `ES 9006` 07:31, `ES 9008` 08:01, `ES 9014` 09:31, `ES 9018` 10:31, `ES 9022` 11:31, `ES 9024` 12:31. Realtime Trains snippets independently corroborated the real `9Ixx` headcode convention on the Brussels route.

**Confirmed retention windows** (correcting an earlier assumption): `schedule_destination_departures` spans 2026-09-08→2026-09-19 (not a trailing window — recent days plus ~7 forward); `schedule_line_population` spans 2026-09-05→2026-09-12; `trust_event_backlog` only had ~3 days actually populated (2026-09-10→2026-09-12), shorter than previously assumed.

**`STP` (the shared domestic TIPLOC) — the one blind spot Round 2 left, now closed and directly refuted as a hiding place:**
- `schedule_destination_departures`'s `STP` 06:00-07:00 slots for 2026-09-12 are exactly the ordinary EMR/Thameslink CIF UIDs you'd expect (`W70942`→Sheffield, `C21551`→Nottingham, `C41852`→Corby, etc.) — no unexplained slot.
- Every `STP`-CRS row in `trust_event_backlog` for 2026-09-12, 05:00-14:08 (spanning 7 of that day's real Eurostar departure times) was pulled and checked — 197 rows. A strict headcode regex (`train_id ~ '9[IOS][0-9]{2}'`) matched **zero**. A loose `LIKE '%9%'` pass surfaced 46 candidates, manually inspected: **100% false positives**, ordinary domestic UIDs that merely contain a "9" digit somewhere.

**Re-confirmed negatives elsewhere**: `EBD`/`AFK`/`SFA` (Stratford International) also show zero rows in `trust_event_backlog`, extending the schedule-only negative to the TRUST/movement layer too. A full-text search of `schedule_line_population`'s population JSON across all 672 line-population rows (every line, not just Eurostar-adjacent ones) found zero occurrences of "eurostar" anywhere.

**The one genuine positive signal, and what it actually pins down.** The `tocs` table *does* have a real, correctly-coded Eurostar row: `ES | Eurostar | Eurostar International Ltd | atoc_member=false | station_operator=true`, and the `stations` table's `SPX` row carries `station_operator = 'ES'`, consistent with it. Both come from RDM's station/TOC *reference* feeds — a different feed from the CIF schedule / TRUST movement feeds. This rules out one hypothesis outright: **this app's RDM subscription is not blind to Eurostar's existence.** The exclusion is specific to the two pipelines that would actually let this app track or status a Eurostar train (CIF full-schedule feed, TRUST movement feed) — consistent with those specific Network Rail products being built for GB passenger/freight TOC settlement and train-describer tracking, and simply not carrying Eurostar's international workings, rather than any code-level filter or subscription-tier restriction on this app's side.

## Conclusion

A real, externally-verified Eurostar service (today's actual departures, cross-checked against the correct station code including the one this research initially missed) is absent from every table this app's CIF/TRUST pipeline could plausibly populate for it — schedule data, movement data, backlog data — under every station coding tried (`SPX`, `EBD`, `AFK`, `SFA`, and `STP`). The reference-data layer (`tocs`, `stations`) correctly knows Eurostar exists as an operator; the operational feeds this app actually ingests for schedule/movement tracking do not carry it at all.

**For anyone revisiting this later**: this is not something fixable by writing more ingestion code or requesting a different RDM subscription tier for *this specific product* — the CIF full-schedule and TRUST movement feeds themselves, industry-wide, are built around GB domestic settlement and train-describer tracking and do not appear to carry Eurostar's international workings as a matter of what those specific feeds contain, not as a filter this app or its subscription applies. A different data source entirely (Eurostar's own public API/timetable, if one is ever integrated) would be a new, separate integration, not an extension of the existing CIF/TRUST-based pipeline.
