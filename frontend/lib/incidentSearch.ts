import { nowInLondon } from '@/lib/londonWallClock';
import type { IncidentSummary } from '@/lib/types';

/** The incident archive's filter state and row text, for
 * `IncidentSearchForm` and `IncidentResults`. */

export type DatePreset = '7d' | '30d' | '90d' | 'all';

/** The Status filter: `GET /public/incidents`' `state`, or no filter.
 * "Upcoming" is listed but not in effect yet; "Ended" is an incident the
 * Knowledgebase feed stopped listing without clearing it, or whose every
 * validity window has finished (see `incidentState` in `lib/incidents.ts`). */
export type StateFilter = 'all' | 'active' | 'upcoming' | 'ended' | 'cleared';

/** The Status filter a URL restores: `?state=` when it names a real state,
 * else the legacy `?cleared=` boolean (`false` = active, `true` = cleared),
 * else All. Malformed means absent, as for the priority params. */
export function initialStateFilter(state: string | undefined, cleared: string | undefined): StateFilter {
  if (state === 'active' || state === 'upcoming' || state === 'ended' || state === 'cleared') return state;
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
 * `lib/trainSearch.ts`'s `TrainSearchResults` type -- `nextCursor` lives INSIDE
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
  | {
      rows: IncidentSummary[];
      nextCursor: string | null;
      query: string;
      loadMoreFailed: boolean;
      /** Every incident the search matches (`GET /public/incidents`'
       * `total`, first page only), or `null` from an api that predates it. */
      total: number | null;
    }
  | 'error'
  | null;

/** The filter state `IncidentSearchForm` searches with and keeps in the
 * URL. Dates are London calendar days (`YYYY-MM-DD`). */
export interface IncidentFilters {
  query: string;
  operators: readonly string[];
  lineId: string | null;
  fromDate: string | null;
  toDate: string | null;
  preset: DatePreset | null;
  planned: 'all' | 'planned' | 'realtime';
  state: StateFilter;
  priorityMin: number | '';
  priorityMax: number | '';
}

/** The URL params `IncidentSearchForm` owns (everything else in the URL is
 * left alone). */
export const INCIDENT_URL_PARAMS = [
  'q',
  'operator',
  'line',
  'from',
  'to',
  'period',
  'planned',
  'state',
  'cleared',
  'priority_min',
  'priority_max',
] as const;

/** `GET /public/incidents`' query for `filters`. `fromDate`/`toDate` name
 * London calendar days (FE-5), so `to` gets the last millisecond of its
 * day: the api's `to` is an inclusive `first_seen_at <= to`. `state`, not
 * the legacy `cleared` boolean, which can only say "not cleared" and so
 * can't ask for "Ended". */
export function incidentSearchParams(
  filters: IncidentFilters,
  dayStartIso: (day: string) => string,
  dayEndIso: (day: string) => string,
): URLSearchParams {
  const params = new URLSearchParams();
  const query = filters.query.trim();
  if (query) params.set('q', query);
  if (filters.operators.length > 0) params.set('operator', filters.operators.join(','));
  if (filters.lineId) params.set('line', filters.lineId);
  if (filters.fromDate) params.set('from', dayStartIso(filters.fromDate));
  if (filters.toDate) params.set('to', dayEndIso(filters.toDate));
  if (filters.planned === 'planned') params.set('planned', 'true');
  if (filters.planned === 'realtime') params.set('planned', 'false');
  if (filters.state !== 'all') params.set('state', filters.state);
  if (filters.priorityMin !== '') params.set('priority_min', String(filters.priorityMin));
  if (filters.priorityMax !== '') params.set('priority_max', String(filters.priorityMax));
  return params;
}

/** The URL for `filters`. A preset period is written as `period` (none
 * for the 30-day default), not as dates, so a shared link to "the last 7
 * days" stays the last 7 days. Custom dates are written as `from`/`to`
 * London days, with `period=custom` when there is no lower bound: a URL
 * with neither restores the 30-day default. `period` exists only in the
 * URL; the api never sees it. */
export function incidentUrlParams(
  filters: IncidentFilters,
  dayStartIso: (day: string) => string,
  dayEndIso: (day: string) => string,
): URLSearchParams {
  const params = incidentSearchParams(filters, dayStartIso, dayEndIso);
  if (filters.preset !== null) {
    params.delete('from');
    params.delete('to');
    if (filters.preset !== '30d') params.set('period', filters.preset);
  } else {
    if (filters.fromDate) params.set('from', filters.fromDate);
    if (filters.toDate) params.set('to', filters.toDate);
    if (!filters.fromDate) params.set('period', 'custom');
  }
  return params;
}

/** The period a URL restores: `period` when it names one (`all` is also
 * what links from before the other presets carried), else custom dates
 * when `from` is set (older links wrote every search as dates), else the
 * 30-day default. `from`/`to` may be a London day or a full timestamp. */
export function initialIncidentPeriod(
  period: string | undefined,
  from: string | undefined,
  to: string | undefined,
  toDay: (value: string) => string,
): { preset: DatePreset | null; fromDate: string | null; toDate: string | null } {
  if (period === '7d' || period === '30d' || period === '90d') {
    return { preset: period, fromDate: calendarDaysAgo(PRESET_DAYS[period]), toDate: null };
  }
  if (period === 'all') return { preset: 'all', fromDate: null, toDate: null };
  if (period === 'custom' || from) {
    return { preset: null, fromDate: from ? toDay(from) : null, toDate: to ? toDay(to) : null };
  }
  return { preset: '30d', fromDate: calendarDaysAgo(30), toDate: null };
}

/** How many days back each preset period reaches. */
export const PRESET_DAYS: Record<Exclude<DatePreset, 'all'>, number> = { '7d': 7, '30d': 30, '90d': 90 };

/** How many filters in the panel differ from the default view (the last 30
 * days, everything else open): the "Filters" button's badge. */
export function activeIncidentFilterCount(filters: IncidentFilters): number {
  return (
    filters.operators.length +
    (filters.lineId ? 1 : 0) +
    (filters.preset === '30d' ? 0 : 1) +
    (filters.planned === 'all' ? 0 : 1) +
    (filters.state === 'all' ? 0 : 1) +
    (filters.priorityMin !== '' || filters.priorityMax !== '' ? 1 : 0)
  );
}

/** The browser tab title for a filtered archive, or `null` for the default
 * view: the line, then operators by name, then the search in quotes. */
export function incidentsTitle(
  filters: Pick<IncidentFilters, 'query' | 'operators' | 'lineId'>,
  lineNamesById: ReadonlyMap<string, string>,
  tocNamesByCode: ReadonlyMap<string, string>,
): string | null {
  const parts: string[] = [];
  if (filters.lineId) parts.push(lineNamesById.get(filters.lineId) ?? filters.lineId);
  const [onlyOperator] = filters.operators;
  if (filters.operators.length > 1) parts.push(`${filters.operators.length} operators`);
  else if (onlyOperator) parts.push(tocNamesByCode.get(onlyOperator) ?? onlyOperator);
  const query = filters.query.trim();
  if (query) parts.push(`"${query}"`);
  return parts.length > 0 ? `Incidents: ${parts.join(', ')}` : null;
}
