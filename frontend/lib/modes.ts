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
 * Mirrors `TFL_TO_NR_LINE_ID` in `crates/common/src/lib.rs`. Elizabeth
 * line is the only entry today; see
 * docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
 * Area 1. */
export const MERGED_TFL_LINE_IDS: readonly string[] = ['tfl-elizabeth'];
