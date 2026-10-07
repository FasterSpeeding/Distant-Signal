/**
 * Pure helpers for the line page's "Trains on this line" section
 * (docs/superpowers/specs/2026-10-06-line-page-trains-design.md): the time
 * window and its Earlier/Later links, direction tabs, the shared-train
 * group, hub links, pattern grouping and the stop strip. Kept free of React
 * so each rule is unit-tested on its own.
 *
 * Times are minutes after the service date's midnight, UK local time, the
 * same scale as the API's `lineDue` (`dayOffset * 1440` + the time), so a
 * 00:20 train of the night is 1460, after 23:50 (1430).
 */
import { isTimetableOnly } from './serviceMode';
import type {
  LineCatalogueStation,
  LineDirection,
  LineTime,
  LineTrainSummary,
  LineTrainSummaryLive,
  ServiceMode,
} from './types';

export const DAY_MINUTES = 24 * 60;
/** The latest minute the API accepts for a window bound (47:59). */
export const MAX_MINUTE = 2 * DAY_MINUTES - 1;

/** The default window: from 30 minutes before `at` to two hours after it
 * on a desktop, one hour on a phone (user decision, 2026-10-06). The page
 * fetches the desktop window once; rows past the phone window are hidden
 * by CSS on narrow screens, so the page works without JavaScript. */
export const WINDOW_BEFORE_MINUTES = 30;
export const WINDOW_AFTER_DESKTOP_MINUTES = 120;
export const WINDOW_AFTER_PHONE_MINUTES = 60;

/** "HH:MM" (hours 00-47) as minutes, or `null` when malformed. */
export function parseMinute(raw: string | null | undefined): number | null {
  if (!raw) return null;
  const match = /^(\d{2}):(\d{2})$/.exec(raw.trim());
  if (!match) return null;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 47 || minutes > 59) return null;
  return hours * 60 + minutes;
}

/** Minutes as the API's "HH:MM" (hours may pass 23 for the next morning). */
export function formatApiMinute(minute: number): string {
  const clamped = Math.max(0, Math.min(minute, MAX_MINUTE + 1));
  const hours = Math.floor(clamped / 60);
  return `${String(hours).padStart(2, '0')}:${String(clamped % 60).padStart(2, '0')}`;
}

/** Minutes as a clock time a traveller reads ("00:20", not "24:20"). */
export function formatClock(minute: number): string {
  const inDay = ((minute % DAY_MINUTES) + DAY_MINUTES) % DAY_MINUTES;
  return `${String(Math.floor(inDay / 60)).padStart(2, '0')}:${String(inDay % 60).padStart(2, '0')}`;
}

/** A `LineTime` as minutes. */
export function lineTimeMinute(time: LineTime | null | undefined): number | null {
  if (!time) return null;
  const minute = parseMinute(time.time);
  return minute === null ? null : minute + time.dayOffset * DAY_MINUTES;
}

/** Minutes after London midnight for `now`. */
export function londonMinuteOfDay(now: Date): number {
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: 'Europe/London',
    hourCycle: 'h23',
    hour: '2-digit',
    minute: '2-digit',
  }).formatToParts(now);
  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? '0');
  return get('hour') * 60 + get('minute');
}

export interface TrainWindow {
  at: number;
  from: number;
  /** End of the desktop window (exclusive). */
  to: number;
  /** End of the phone window (exclusive). */
  phoneTo: number;
  earlierDesktop: number;
  laterDesktop: number;
  earlierPhone: number;
  laterPhone: number;
}

/** The window around `at`, and where Earlier/Later move it. Earlier/Later
 * step by the window's forward length, so consecutive pages overlap only
 * by the 30-minute look-back. Clamped to the service date's range. */
export function resolveWindow(at: number): TrainWindow {
  const clampAt = (m: number) => Math.max(0, Math.min(m, MAX_MINUTE - WINDOW_AFTER_DESKTOP_MINUTES));
  const a = clampAt(at);
  return {
    at: a,
    from: Math.max(0, a - WINDOW_BEFORE_MINUTES),
    to: a + WINDOW_AFTER_DESKTOP_MINUTES,
    phoneTo: a + WINDOW_AFTER_PHONE_MINUTES,
    earlierDesktop: clampAt(a - WINDOW_AFTER_DESKTOP_MINUTES),
    laterDesktop: clampAt(a + WINDOW_AFTER_DESKTOP_MINUTES),
    earlierPhone: clampAt(a - WINDOW_AFTER_PHONE_MINUTES),
    laterPhone: clampAt(a + WINDOW_AFTER_PHONE_MINUTES),
  };
}

/** The page's own URL parameters, sanitised. Anything malformed is
 * dropped, never trusted into an API call. */
export interface LinePageParams {
  dir: LineDirection | null;
  at: number | null;
  from: string | null;
  to: string | null;
  view: 'routes' | null;
}

function first(value: string | string[] | undefined): string | undefined {
  return Array.isArray(value) ? value[0] : value;
}

export function parseLinePageParams(raw: Record<string, string | string[] | undefined>): LinePageParams {
  const dir = first(raw.dir);
  const crs = (value: string | undefined) => {
    const upper = value?.trim().toUpperCase();
    return upper && /^[A-Z]{3}$/.test(upper) ? upper : null;
  };
  return {
    dir: dir === 'up' || dir === 'down' || dir === 'loop' ? dir : null,
    at: parseMinute(first(raw.at)),
    from: crs(first(raw.from)),
    to: crs(first(raw.to)),
    view: first(raw.view) === 'routes' ? 'routes' : null,
  };
}

/** `/lines/{id}` with `params` (the current ones, overridden), dropping
 * empty values -- every link on the section is built here so they keep
 * each other's state. */
export function lineHref(id: string, params: Partial<Record<keyof LinePageParams, string | null>>): string {
  const query = new URLSearchParams();
  for (const key of ['dir', 'at', 'from', 'to', 'view'] as const) {
    const value = params[key];
    if (value) query.set(key, value);
  }
  const qs = query.toString();
  return `/lines/${encodeURIComponent(id)}${qs ? `?${qs}` : ''}#trains`;
}

/** The current params as `lineHref` input. */
export function paramsForHref(params: LinePageParams): Partial<Record<keyof LinePageParams, string | null>> {
  return {
    dir: params.dir,
    at: params.at === null ? null : formatApiMinute(params.at),
    from: params.from,
    to: params.to,
    view: params.view,
  };
}

/** Sort key: time on the line, then uid. A train without one sorts last. */
export function sortByLineTime(trains: LineTrainSummary[]): LineTrainSummary[] {
  return [...trains].sort((a, b) => {
    const am = lineTimeMinute(a.lineDue) ?? Number.MAX_SAFE_INTEGER;
    const bm = lineTimeMinute(b.lineDue) ?? Number.MAX_SAFE_INTEGER;
    return am - bm || a.uid.localeCompare(b.uid);
  });
}

export function stationName(crs: string, stations: LineCatalogueStation[], fallback?: string | null): string {
  return stations.find((s) => s.crs === crs)?.name ?? fallback ?? crs;
}

/** "Towards <terminus>" for up/down (the catalogue's first station for
 * `up`, its last for `down`), "Loop" for loop trains. */
export function directionLabel(direction: LineDirection, stations: LineCatalogueStation[]): string {
  if (direction === 'loop') return 'Loop';
  const end = direction === 'up' ? stations[0] : stations[stations.length - 1];
  return end ? `Towards ${end.name ?? end.crs}` : direction === 'up' ? 'Up' : 'Down';
}

export interface DirectionTab {
  dir: LineDirection | null;
  label: string;
  count: number;
}

/** The direction tabs: "All", then up and down (labelled by terminus),
 * then "Loop" only when loop trains run in the window. Counts are the
 * line's own trains in the window. A line whose population predates
 * directions gets no tabs at all. */
export function directionTabs(
  counts: Partial<Record<string, Partial<Record<string, number>>>>,
  stations: LineCatalogueStation[],
  scopeApplied: boolean,
): DirectionTab[] {
  if (!scopeApplied) return [];
  const own = counts.line ?? {};
  const n = (d: string) => own[d] ?? 0;
  const total = Object.values(own).reduce<number>((sum, c) => sum + (c ?? 0), 0);
  const tabs: DirectionTab[] = [
    { dir: null, label: 'All', count: total },
    { dir: 'up', label: directionLabel('up', stations), count: n('up') },
    { dir: 'down', label: directionLabel('down', stations), count: n('down') },
  ];
  if (n('loop') > 0) tabs.push({ dir: 'loop', label: 'Loop', count: n('loop') });
  return tabs;
}

/** Is `train` running on the line? The API's `running` list already says
 * so; this splits that list from the window list without duplicates. */
export function splitRunning(
  trains: LineTrainSummary[],
  running: LineTrainSummary[] | null,
): { running: LineTrainSummary[]; upcoming: LineTrainSummary[] } {
  const own = (running ?? []).filter((t) => t.scope !== 'shared');
  const ids = new Set(own.map((t) => t.uid));
  return { running: own, upcoming: trains.filter((t) => !ids.has(t.uid)) };
}

export interface SharedGroup {
  key: string;
  operator: string;
  route: string;
  trains: LineTrainSummary[];
}

/** Shared trains grouped by operator and route ("CrossCountry · Manchester
 * Piccadilly → Bournemouth"), groups in order of their first train. */
export function groupShared(trains: LineTrainSummary[], operatorName: (code: string) => string): SharedGroup[] {
  const groups = new Map<string, SharedGroup>();
  for (const train of sortByLineTime(trains)) {
    const operator = train.operator ? operatorName(train.operator) : 'Unknown operator';
    const route = `${train.origin?.name ?? train.origin?.crs ?? '?'} → ${
      train.destination?.name ?? train.destination?.crs ?? '?'
    }`;
    const key = `${train.operator ?? ''}|${train.origin?.crs ?? ''}|${train.destination?.crs ?? ''}`;
    const group = groups.get(key) ?? { key, operator, route, trains: [] };
    group.trains.push(train);
    groups.set(key, group);
  }
  return [...groups.values()];
}

/** The line's hub stations for "Other trains at …" links: its termini
 * and junctions, where most of the trains that only touch the line call.
 * Termini only when the catalogue marks no junction. */
export function hubStations(stations: LineCatalogueStation[]): LineCatalogueStation[] {
  const hubs = stations.filter((s) => s.role === 'terminus' || s.role === 'junction');
  const unique = hubs.filter((s, i) => hubs.findIndex((h) => h.crs === s.crs) === i);
  return unique.length > 0 ? unique : stations.slice(0, 1);
}

/** Key stations for the stop strip: termini and major stations. */
const KEY_ROLES = new Set(['terminus', 'major']);

export interface StopStrip {
  /** Key stations after the row's own (first) stop, by name. */
  shown: string[];
  /** On-line stops not shown. */
  hidden: number;
  /** The text alternative: every on-line stop after the first. */
  text: string;
}

/** The compact on-line stop strip: key stations (catalogue `role`
 * terminus/major) after the first stop, the rest as "+N stops", and a
 * text alternative naming them all. When a train calls at no key station,
 * its last stop stands in. */
export function stopStrip(train: LineTrainSummary, stations: LineCatalogueStation[]): StopStrip {
  const after = train.onLineStops.slice(1);
  const role = (crs: string) => stations.find((s) => s.crs === crs)?.role;
  let shown = after.filter((s) => KEY_ROLES.has(role(s.crs) ?? ''));
  const last = after.at(-1);
  if (shown.length === 0 && last) shown = [last];
  const names = after.map((s) => stationName(s.crs, stations));
  const text =
    names.length === 0
      ? 'No further stops on this line'
      : names.length === 1
        ? `Then calls at ${names[0]}`
        : `Then calls at ${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
  return {
    shown: shown.map((s) => stationName(s.crs, stations)),
    hidden: after.length - shown.length,
    text,
  };
}

export interface PatternGroup {
  key: string;
  /** "London Waterloo → Weymouth" */
  route: string;
  /** "fast" / "semi-fast" / "stopping" when the route has several
   * stopping patterns; null otherwise. */
  kind: string | null;
  /** "every 30 min · xx:05, xx:35", or "3 trains" when irregular. */
  frequency: string;
  trains: LineTrainSummary[];
}

/** "every 30 min · xx:05, xx:35" for evenly spaced times (a 1-minute
 * tolerance), the minutes past the hour only when the interval divides an
 * hour; "N trains" otherwise. */
export function frequencySummary(minutes: number[]): string {
  const sorted = [...minutes].sort((a, b) => a - b);
  const count = `${sorted.length} train${sorted.length === 1 ? '' : 's'}`;
  if (sorted.length < 3) return count;
  const gaps = sorted.slice(1).map((m, i) => m - (sorted[i] ?? m));
  const interval = Math.round(gaps.reduce((a, b) => a + b, 0) / gaps.length);
  if (interval < 5 || gaps.some((g) => Math.abs(g - interval) > 1)) return count;
  if (60 % interval !== 0) return `every ${interval} min`;
  // Minutes past the hour of the trains themselves, a minute's drift
  // folded into the earlier one (xx:02 and xx:03 read as xx:02).
  const distinct: number[] = [];
  for (const past of sorted.map((m) => m % 60).sort((x, y) => x - y)) {
    const prev = distinct.at(-1);
    if (prev === undefined || past - prev > 1) distinct.push(past);
  }
  return `every ${interval} min · ${distinct.map((m) => `xx:${String(m).padStart(2, '0')}`).join(', ')}`;
}

/** Trains grouped by stopping pattern: same origin, destination and
 * on-line stops. Patterns sharing a route are told apart as fast,
 * semi-fast and stopping by how many stops they make. Groups in order of
 * their first train. */
export function groupPatterns(trains: LineTrainSummary[]): PatternGroup[] {
  const byPattern = new Map<string, LineTrainSummary[]>();
  for (const train of sortByLineTime(trains)) {
    const key = [
      train.origin?.crs ?? '',
      train.destination?.crs ?? '',
      train.onLineStops.map((s) => s.crs).join('-'),
    ].join('|');
    byPattern.set(key, [...(byPattern.get(key) ?? []), train]);
  }
  const routeKey = (t: LineTrainSummary) => `${t.origin?.crs ?? ''}|${t.destination?.crs ?? ''}`;
  const stopCounts = new Map<string, number[]>();
  for (const [head] of byPattern.values()) {
    if (!head) continue;
    const rk = routeKey(head);
    stopCounts.set(rk, [...(stopCounts.get(rk) ?? []), head.onLineStops.length]);
  }
  return [...byPattern.entries()].flatMap(([key, group]) => {
    const head = group[0];
    if (!head) return [];
    const counts = [...new Set(stopCounts.get(routeKey(head)) ?? [])].sort((a, b) => a - b);
    const stops = head.onLineStops.length;
    const kind =
      counts.length < 2 ? null : stops === counts[0] ? 'fast' : stops === counts.at(-1) ? 'stopping' : 'semi-fast';
    const minutes = group.map((t) => lineTimeMinute(t.lineDue)).filter((m): m is number => m !== null);
    return [
      {
        key,
        route: `${head.origin?.name ?? head.origin?.crs ?? '?'} → ${head.destination?.name ?? head.destination?.crs ?? '?'}`,
        kind,
        frequency: frequencySummary(minutes),
        trains: group,
      },
    ];
  });
}

export type LiveTone = 'cancelled' | 'late' | 'onTime' | 'timetable' | 'none';

/** The row's status text and tone: cancelled, n min late, on time, or
 * nothing live yet. Buses and ferries are never reported live. */
export function liveStatusLabel(
  live: LineTrainSummaryLive | null,
  serviceMode: ServiceMode | null | undefined,
): { text: string; tone: LiveTone } {
  if (live?.cancelled || live?.status === 'cancelled') return { text: 'Cancelled', tone: 'cancelled' };
  if (isTimetableOnly({ serviceMode })) return { text: 'Timetable only', tone: 'timetable' };
  if (live?.delayMinutes != null) {
    const prefix = live.delayProvisional ? 'Exp. ' : '';
    if (live.delayMinutes > 0) return { text: `${prefix}${live.delayMinutes} min late`, tone: 'late' };
    return { text: 'On time', tone: 'onTime' };
  }
  return { text: 'Scheduled', tone: 'none' };
}

/** The query of `GET /public/lines/{id}/timetable`, shared by the server
 * fetch below and the page's client-side "Load more" (through the
 * `/api` proxy). Empty values are left out. */
export function lineTimetableQuery(options: {
  date: string;
  dir?: LineDirection | null;
  from?: string | null;
  to?: string | null;
  at?: string | null;
  scope?: string | null;
  after?: string | null;
  limit?: number;
}): string {
  const params = new URLSearchParams({ date: options.date });
  if (options.scope) params.set('scope', options.scope);
  if (options.dir) params.set('dir', options.dir);
  if (options.from) params.set('from', options.from);
  if (options.to) params.set('to', options.to);
  if (options.at) params.set('at', options.at);
  if (options.after) params.set('after', options.after);
  if (options.limit !== undefined) params.set('limit', String(options.limit));
  return params.toString();
}

/** Which trains the full-day timetable lists: the line's own (default),
 * or other trains running a stretch of it. */
export type TimetableScope = 'line' | 'shared';

/** `/lines/{id}/timetable`'s URL parameters, sanitised (anything
 * malformed is dropped, never passed to the API). */
export interface TimetablePageParams {
  /** `YYYY-MM-DD`; `null` for today. */
  date: string | null;
  dir: LineDirection | null;
  from: string | null;
  to: string | null;
  /** First time listed (minutes after the date's midnight). */
  at: number | null;
  scope: TimetableScope;
  /** A `nextCursor` (the no-JavaScript "next trains" link). */
  after: string | null;
}

function validDate(raw: string | undefined): string | null {
  if (!raw || !/^\d{4}-\d{2}-\d{2}$/.test(raw)) return null;
  const parsed = new Date(`${raw}T00:00:00Z`);
  return Number.isNaN(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== raw ? null : raw;
}

export function parseTimetableParams(raw: Record<string, string | string[] | undefined>): TimetablePageParams {
  const line = parseLinePageParams(raw);
  const after = first(raw.after);
  return {
    date: validDate(first(raw.date)),
    dir: line.dir,
    from: line.from,
    to: line.to,
    at: line.at,
    scope: first(raw.scope) === 'shared' ? 'shared' : 'line',
    after: after && /^\d{1,4}\.[A-Za-z0-9-]{1,16}$/.test(after) ? after : null,
  };
}

/** `/lines/{id}/timetable` with `params`, empty values (and the default
 * scope) left out. */
export function timetableHref(id: string, params: Partial<Record<keyof TimetablePageParams, string | null>>): string {
  const query = new URLSearchParams();
  for (const key of ['date', 'dir', 'from', 'to', 'at', 'scope', 'after'] as const) {
    const value = params[key];
    if (value && !(key === 'scope' && value === 'line')) query.set(key, value);
  }
  const qs = query.toString();
  return `/lines/${encodeURIComponent(id)}/timetable${qs ? `?${qs}` : ''}`;
}

/** The current timetable params as `timetableHref` input (no cursor: a
 * filter change starts from the top). */
export function timetableParamsForHref(
  params: TimetablePageParams,
): Partial<Record<keyof TimetablePageParams, string | null>> {
  return {
    date: params.date,
    dir: params.dir,
    from: params.from,
    to: params.to,
    at: params.at === null ? null : formatApiMinute(params.at),
    scope: params.scope,
  };
}

/** The timetable link on the line page: the line page's direction and
 * stations, from the start of its current window. */
export function lineTimetableLink(id: string, params: LinePageParams, windowFrom: number): string {
  return timetableHref(id, {
    dir: params.dir,
    from: params.from,
    to: params.to,
    at: formatApiMinute(windowFrom),
  });
}

/** Until when (minutes after midnight) the line page also shows the
 * previous service date's trains: those still running after midnight,
 * which belong to the date they started on. 03:00 covers every overnight
 * run's tail on a line. */
export const OVERNIGHT_UNTIL_MINUTES = 3 * 60;

/** The window and `at` on the previous service date's scale (+24 h), for
 * a window that starts before {@link OVERNIGHT_UNTIL_MINUTES}; `null`
 * otherwise. Bounds stay within the API's 47:59. */
export function previousDayWindow(
  window: TrainWindow,
  at: number | null,
): { from: number; to: number; at: number | null } | null {
  if (window.from >= OVERNIGHT_UNTIL_MINUTES) return null;
  return {
    from: window.from + DAY_MINUTES,
    to: Math.min(window.to + DAY_MINUTES, MAX_MINUTE),
    at: at === null ? null : Math.min(at + DAY_MINUTES, MAX_MINUTE),
  };
}

/** A previous-date train on today's scale: every day offset one less, so
 * its 00:20 (offset 1) sorts among today's 00:20 (offset 0). */
export function shiftToNextDay(train: LineTrainSummary): LineTrainSummary {
  return {
    ...train,
    lineDue: train.lineDue ? { ...train.lineDue, dayOffset: train.lineDue.dayOffset - 1 } : null,
    onLineStops: train.onLineStops.map((s) => ({ ...s, dayOffset: s.dayOffset - 1 })),
  };
}

/** `YYYY-MM-DD` minus one calendar day. */
export function previousDate(date: string): string {
  const d = new Date(`${date}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() - 1);
  return d.toISOString().slice(0, 10);
}

/** Two summaries' counts added up. */
export function addCounts(
  a: Partial<Record<string, Partial<Record<string, number>>>>,
  b: Partial<Record<string, Partial<Record<string, number>>>>,
): Partial<Record<string, Partial<Record<string, number>>>> {
  const out: Partial<Record<string, Partial<Record<string, number>>>> = {};
  for (const counts of [a, b]) {
    for (const [scope, byDir] of Object.entries(counts)) {
      const target = (out[scope] ??= {});
      for (const [dir, n] of Object.entries(byDir ?? {})) target[dir] = (target[dir] ?? 0) + (n ?? 0);
    }
  }
  return out;
}
