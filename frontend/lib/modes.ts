/** Every mode whose lines this app displays.
 *
 * Mirrors `SUPPORTED_MODES` in `crates/api/src/routes/line_status.rs` —
 * that list is a closed set, so a mode missing from it comes back as a 400
 * rather than an empty array. `national-rail` is computed by the aggregator
 * from Knowledgebase incidents and LDBWS samples; the other five are
 * published by TfL and ingested wholesale by `crates/poller-tfl`.
 *
 * Kept as one constant rather than a per-page literal because both list
 * pages need the same set, and a page that quietly omits a mode looks
 * exactly like a mode with no disruptions. */
export const DISPLAYED_MODES = [
  'national-rail',
  'tube',
  'dlr',
  'overground',
  'elizabeth-line',
  'tram',
] as const;

/** The value to interpolate into `/Line/Mode/{modes}/Status`. TfL's own API
 * takes a comma-separated list here and this one mimics it, so all six
 * modes are one round trip. */
export const DISPLAYED_MODES_PARAM = DISPLAYED_MODES.join(',');

/** TfL line ids that have a National Rail counterpart and so are folded
 * into that counterpart's row everywhere a line list is built directly
 * from ids/reports rather than from `/public/lines` (which already omits
 * them — see `crates/api/src/routes/lines.rs::is_merged_into_nr_line`).
 * Mirrors `TFL_TO_NR_LINE_ID` in `crates/common/src/lib.rs`. Elizabeth line
 * is Area 1; the six London Overground lines are Area 2, added once
 * `lines/overground-*.toml` existed for them — see
 * docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
 * Areas 1 and 2. */
export const MERGED_TFL_LINE_IDS: readonly string[] = [
  'tfl-elizabeth',
  'tfl-liberty',
  'tfl-lioness',
  'tfl-mildmay',
  'tfl-suffragette',
  'tfl-weaver',
  'tfl-windrush',
];

/** The three jurisdictions this app can ever attribute a line-status row
 * to. Mirrors `IslandOfIrelandNetwork` in `crates/common/src/lib.rs`
 * (`NorthernIreland`/`RepublicOfIreland`) exactly, with `Gb` added as the
 * implicit "not tagged Ireland" default rather than a fourth variant
 * anywhere in `common::` -- see
 * docs/superpowers/specs/2026-09-05-country-filtering-design.md Decision 1
 * and Decision 2 for why GB is never an explicit backend tag. */
export type Country = 'Gb' | 'NorthernIreland' | 'RepublicOfIreland';

/** Maps a `LineStatusReport.modeName` to the country it belongs to.
 * Deliberately empty today: every mode this app can currently emit a
 * report for (`DISPLAYED_MODES`, above) is GB, and GB is never listed here
 * explicitly -- a `modeName` absent from this table is `Gb` by
 * construction (see Decision 2). This table gains entries only once a
 * real non-GB poller exists and its real `modeName` value(s) are known --
 * see docs/superpowers/specs/2026-09-05-country-filtering-design.md
 * Decision 3 and §8 Open Question 2, and
 * docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md:196 for
 * why `island-of-ireland-*` is illustrative, not a committed value, and
 * must not be guessed at here. Mirrors `MERGED_TFL_LINE_IDS`'s own shape:
 * a small, hand-maintained lookup over a mode/id this app already emits,
 * not a new backend field. */
export const MODE_TO_COUNTRY: Record<string, Country> = {};

/** Derives a country from a raw `modeName` string. `table` defaults to the
 * real `MODE_TO_COUNTRY` but is overridable so this derivation can be unit
 * tested against a synthetic mapping before any real non-GB `modeName`
 * exists (see the design spec's Judgment Call 2 in
 * docs/superpowers/plans/2026-09-05-country-filtering-plan.md) --
 * production call sites should never pass `table` explicitly. */
export function countryForMode(modeName: string, table: Record<string, Country> = MODE_TO_COUNTRY): Country {
  return table[modeName] ?? 'Gb';
}

/** `countryForMode`, keyed off a `LineStatusReport`/`LineStatusHistoryEntry`
 * directly -- `Pick<..., 'modeName'>` rather than the full report type so a
 * caller (or a test) doesn't need to fabricate every other required field
 * just to derive a country. */
export function countryForReport(
  report: Pick<import('./types').LineStatusReport, 'modeName'>,
  table: Record<string, Country> = MODE_TO_COUNTRY,
): Country {
  return countryForMode(report.modeName, table);
}
