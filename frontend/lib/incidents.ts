const KNOWLEDGEBASE_INCIDENT_PREFIX = 'knowledgebase-incident-';

/** The only place in this frontend that "parses" `Disruption.source` — see
 * docs/superpowers/specs/2026-08-31-incident-detail-page-design.md
 * Correction 1 for why this exact prefix, and why the LDBWS
 * ('ldbws-sampling', a shared literal constant, not an id) and TfL
 * ('tfl-line-status-{lineId}', keyed off a line id, not an incident id)
 * source values must NOT resolve to a link — neither names a real
 * `incidents` row, so there is nothing for `/incidents/[id]` to show for
 * either. */
export function incidentIdFromSource(source: string | null | undefined): string | null {
  if (!source || !source.startsWith(KNOWLEDGEBASE_INCIDENT_PREFIX)) return null;
  return source.slice(KNOWLEDGEBASE_INCIDENT_PREFIX.length);
}
