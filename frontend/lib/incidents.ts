import { formatDateTime } from './dateFormat';
import type { IncidentTiming, ValidityPeriod } from './types';
import { validityPhase } from './validity';

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

/** An incident's state (2026-10-06,
 * docs/superpowers/specs/2026-10-06-incident-source-removal-design.md;
 * validity windows since 2026-10-20,
 * docs/superpowers/specs/2026-10-20-incident-validity-state-design.md),
 * first match wins:
 * - `cleared`: RDM's own Knowledgebase set `ClearedIncident`;
 * - `ended`: the feed stopped listing it without ever clearing it (it purges
 *   nightly, and never clears planned work), so it is no longer live but
 *   was not "cleared" either -- `sourceRemovedAt` is when it was last listed;
 * - by its validity windows: `active` while one covers now, `upcoming`
 *   while none has started yet, `ended` once every one has finished
 *   (`validityEndedAt`).
 *
 * The api derives this (`state`, one rule shared with its Status filter
 * and counts), and its answer is used whenever it is there. Against an api
 * that predates `state` (a rolling deploy), the same rule runs here on
 * `validityPeriods` when the caller has them (the detail page), else every
 * uncleared, listed row reads as active, exactly as before. */
export type IncidentState = IncidentTiming['state'];

const STATES: readonly IncidentState[] = ['active', 'upcoming', 'ended', 'cleared'];

export function incidentState(
  incident: {
    isCleared: boolean;
    sourceRemovedAt?: string | null | undefined;
    state?: string | null | undefined;
    validityPeriods?: readonly ValidityPeriod[] | undefined;
  },
  now: number = Date.now(),
): IncidentState {
  const served = STATES.find((state) => state === incident.state);
  if (served) return served;
  if (incident.isCleared) return 'cleared';
  if (incident.sourceRemovedAt) return 'ended';
  return incident.validityPeriods ? validityPhase(incident.validityPeriods, now) : 'active';
}

/** The words behind an "Ended" badge for an incident the feed stopped
 * listing. */
export function endedDescription(sourceRemovedAt: string): string {
  return `No longer listed by the source since ${formatDateTime(sourceRemovedAt)}`;
}

/** The words behind an "Ended" badge for an incident still listed whose
 * every validity window has finished. */
export function validityEndedDescription(validityEndedAt: string): string {
  return `Its validity period ended ${formatDateTime(validityEndedAt)}`;
}

/** The words behind an "Upcoming" badge. */
export function upcomingDescription(startsAt: string): string {
  return `Not in effect yet: starts ${formatDateTime(startsAt)}`;
}
