/** Maps an incident's `source` provenance string to a human label.
 *
 * `source` is not a free string: it is one of three prefixed shapes
 * produced by the pipeline -- `knowledgebase-incident-{id}`
 * (crates/aggregator/src/aggregation.rs), `ldbws-sampling`
 * (aggregation.rs) and `tfl-line-status-{lineId}`
 * (crates/poller-tfl/src/schema.rs). The prefix already IS a source
 * enum; nothing had ever mapped it, so `DisruptionDetail` rendered the
 * whole thing, producing "Source:
 * knowledgebase-incident-EC354602568440DB82B2835903B7A5FE" in body copy
 * (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F5).
 *
 * Returns `null` for anything unrecognised, so a new pipeline source
 * renders nothing rather than a raw internal string -- the same fail-safe
 * `lib/impactType.ts` documents. */
export function incidentSourceLabel(source: string | null | undefined): string | null {
  if (!source) return null;
  if (source.startsWith('knowledgebase-incident-')) return 'National Rail Knowledgebase';
  if (source === 'ldbws-sampling') return 'Live departure boards';
  if (source.startsWith('tfl-line-status-')) return 'Transport for London';
  return null;
}
