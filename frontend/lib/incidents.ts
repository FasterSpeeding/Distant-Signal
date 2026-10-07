import { formatDateTime } from './dateFormat';

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
  if (!source?.startsWith(KNOWLEDGEBASE_INCIDENT_PREFIX)) return null;
  return source.slice(KNOWLEDGEBASE_INCIDENT_PREFIX.length);
}

/** An incident's lifecycle state (2026-10-06,
 * docs/superpowers/specs/2026-10-06-incident-source-removal-design.md):
 * - `cleared`: RDM's own Knowledgebase set `ClearedIncident`;
 * - `ended`: the feed stopped listing it without ever clearing it (it purges
 *   nightly, and never clears planned work), so it is no longer live but
 *   was not "cleared" either -- `sourceRemovedAt` is when it was last listed;
 * - `active`: neither.
 * `sourceRemovedAt` is optional so a bundle served against an api that
 * predates the field (a rolling deploy) reads every uncleared row as active,
 * exactly as before. */
export type IncidentState = 'active' | 'cleared' | 'ended';

export function incidentState(incident: {
  isCleared: boolean;
  sourceRemovedAt?: string | null | undefined;
}): IncidentState {
  if (incident.isCleared) return 'cleared';
  if (incident.sourceRemovedAt) return 'ended';
  return 'active';
}

/** The words behind an "Ended" badge. */
export function endedDescription(sourceRemovedAt: string): string {
  return `No longer listed by the source since ${formatDateTime(sourceRemovedAt)}`;
}
