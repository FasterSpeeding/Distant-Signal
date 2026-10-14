import { nowInLondon } from '@/lib/londonWallClock';
import type { IncidentSummary } from '@/lib/types';

/** The incident archive's filter state and row text, for
 * `IncidentSearchForm` and `IncidentResults`. */

export type DatePreset = '7d' | '30d' | '90d' | 'all';

/** The Status filter: `GET /public/incidents`' `state`, or no filter.
 * "Ended" is an incident the Knowledgebase feed stopped listing without
 * clearing it (see `incidentState` in `lib/incidents.ts`). */
export type StateFilter = 'all' | 'active' | 'ended' | 'cleared';

/** The Status filter a URL restores: `?state=` when it names a real state,
 * else the legacy `?cleared=` boolean (`false` = active, `true` = cleared),
 * else All. Malformed means absent, as for the priority params. */
export function initialStateFilter(state: string | undefined, cleared: string | undefined): StateFilter {
  if (state === 'active' || state === 'ended' || state === 'cleared') return state;
  if (cleared === 'false') return 'active';
  if (cleared === 'true') return 'cleared';
  return 'all';
}

/** How many affected-line badges one result row shows before collapsing the
 * rest into a "+N more". An operator-only incident on a large TOC matches
 * every catalogue line that TOC runs -- Northern alone is 13 -- which would
 * otherwise bury the summary under a wall of badges. */
const MAX_LINE_BADGES = 4;

/** "South Western Railway · South Western Main Line, Portsmouth Direct Line
 * +3 more · WOK, GLD": the operators (by name), lines and stations an
 * archived incident affects, as one line of text. */
export function affectsSummary(
  row: Pick<IncidentSummary, 'operators' | 'affectedLines' | 'affectedStations'>,
  tocNamesByCode: ReadonlyMap<string, string>,
  lineNamesById: ReadonlyMap<string, string>,
): string {
  const operators = row.operators.map((code) => tocNamesByCode.get(code) ?? code).join(', ');
  const lines = row.affectedLines ?? [];
  const shownLines = lines.slice(0, MAX_LINE_BADGES).map((id) => lineNamesById.get(id) ?? id);
  const moreLines = lines.length > MAX_LINE_BADGES ? ` +${lines.length - MAX_LINE_BADGES} more` : '';
  const lineText = shownLines.length > 0 ? `${shownLines.join(', ')}${moreLines}` : '';
  return [operators, lineText, row.affectedStations.join(', ')].filter((part) => part !== '').join(' · ');
}

export function calendarDaysAgo(days: number): string {
  return nowInLondon().subtract(days, 'day').format('YYYY-MM-DD');
}

/** Parses `priority_min`/`priority_max`'s raw URL string into the numeric
 * value `priorityMin`/`priorityMax` state expects, folding "absent",
 * "present but blank" (a literal `?priority_min=`) and "not a real, finite
 * number at all" into the same `''` (blank) result `NumberInput` already
 * treats as "no filter" -- same "malformed means absent" posture
 * `app/track/page.tsx`'s `validTimeParam`/`ticketIdParam` already take
 * elsewhere in this codebase, just applied inside the component rather than
 * the page since these two params aren't pre-validated by their caller.
 * `Number.isFinite` rather than `!Number.isNaN`: `Number('')` is `0` (not
 * `NaN`), which the blank-string check above already intercepts, but
 * `Number('Infinity')` is a real, finite-looking `NaN`-free number the
 * NaN-only check would have let straight through as a filter value.
 * Never throws on a garbage value in the URL. */
export function parsePriorityParam(value: string | undefined): number | '' {
  if (value === undefined || value.trim() === '') return '';
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : '';
}

/** Exactly one of three mutually-exclusive states, mirroring
 * `TrainSearchForm.tsx`'s own `Results` type -- `nextCursor` lives INSIDE
 * the success variant for the same reason it does there: it must not
 * survive a state transition (a fresh search, or an error) it does not
 * belong to. `query` is the exact query string that was submitted to produce
 * these `rows` -- captured once at submit time, mirroring
 * `TrainSearchForm.tsx`'s own capture of `date` for the identical reason:
 * `handleLoadMore` must page through THIS search's filters, never whatever
 * live filter state the form happens to hold when "Load more" is pressed.
 *
 * `loadMoreFailed` records that the LAST "Load more" press errored, so the
 * footer can say so instead of pretending the list ended there. It lives in
 * this variant (rather than its own `useState`) for the same reason
 * `nextCursor` does: a fresh search replaces the whole object, so the flag
 * cannot outlive the result set it describes. The cursor is deliberately
 * kept on failure -- it is still a valid cursor, so the retry the footer
 * offers is a real one. */
export type IncidentResults =
  { rows: IncidentSummary[]; nextCursor: string | null; query: string; loadMoreFailed: boolean } | 'error' | null;
