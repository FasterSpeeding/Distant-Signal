import { londonDayKey } from './dateFormat';
import { severityRank } from './severity';
import type { LineStatus, LineStatusHistoryEntry } from './types';

const DAY_MS = 86_400_000;

export interface HistorySpan {
  /** Worst severity across the span's statuses, by true rank. */
  severity: number;
  statuses: LineStatus[];
  /** `computedAt` of the first recompute in the run. */
  from: string;
  /** `computedAt` of the last recompute in the run. */
  to: string;
  /** How many recomputes were collapsed into this span. */
  samples: number;
}

export interface HistoryDay {
  /** `YYYY-MM-DD`, London. */
  day: string;
  spans: HistorySpan[];
}

/** Identity of an entry's *state*, order-insensitive: two recomputes that
 * found the same set of statuses are the same state even if the aggregator
 * happened to emit them in a different order. Severity plus reason is
 * enough — `statusSeverityDescription` is a pure function of severity, and
 * validity periods on a historical snapshot move as `now` moves, which
 * would defeat the collapsing for no benefit.
 *
 * `JSON.stringify` on the sorted `[severity, reason]` pairs, not a
 * delimiter-free string join: joining `${severity} ${reason}` entries with
 * no separator between them let two genuinely different status sets
 * collide at a digit boundary — `[[1,'A2'],[2,'B']]` and `[[1,'A'],[22,'B']]`
 * both produced `"1 A22 B"`, which would have silently merged a real status
 * transition into one span. `JSON.stringify` escapes and delimits its array
 * elements unambiguously, so no two distinct pair sets can produce the same
 * signature. */
function stateSignature(entry: LineStatusHistoryEntry): string {
  return JSON.stringify(
    entry.lineStatuses
      .map((status): [number, string] => [status.statusSeverity, status.reason])
      .sort((a, b) => a[0] - b[0] || a[1].localeCompare(b[1])),
  );
}

function worstSeverity(statuses: LineStatus[]): number {
  return statuses.reduce(
    (worst, status) => (severityRank(status.statusSeverity) > severityRank(worst) ? status.statusSeverity : worst),
    10,
  );
}

/** The aggregator recomputes every 5–15 minutes, so a 30-day history is
 * thousands of entries describing a handful of actual state changes — the
 * page came out 34,659px tall at desktop and ~46,000px at mobile, almost
 * all of it the same sentence repeated. Runs of consecutive recomputes with
 * an identical status set collapse into one span with its own start, end
 * and sample count. Entries are sorted oldest-first first, so a "span" is
 * always a genuinely contiguous run regardless of what order the API
 * returned them in. */
export function collapseHistory(entries: LineStatusHistoryEntry[]): HistorySpan[] {
  const ordered = [...entries].sort((a, b) => Date.parse(a.computedAt) - Date.parse(b.computedAt));

  const spans: HistorySpan[] = [];
  let signature: string | null = null;

  for (const entry of ordered) {
    const next = stateSignature(entry);
    const current = spans[spans.length - 1];
    if (current && next === signature) {
      current.to = entry.computedAt;
      current.samples += 1;
      continue;
    }
    signature = next;
    spans.push({
      severity: worstSeverity(entry.lineStatuses),
      statuses: entry.lineStatuses,
      from: entry.computedAt,
      to: entry.computedAt,
      samples: 1,
    });
  }

  return spans;
}

/** Newest day first, newest span first within a day — the page's main
 * question is "what happened recently", and the previous oldest-first
 * ordering put the most recent state 34,000px below the fold. Keyed on the
 * London calendar day so a summer evening doesn't split across two
 * headings at 23:00 UTC. */
export function groupSpansByDay(spans: HistorySpan[]): HistoryDay[] {
  const byDay = new Map<string, HistorySpan[]>();
  for (const span of spans) {
    const day = londonDayKey(span.from);
    const existing = byDay.get(day);
    if (existing) existing.push(span);
    else byDay.set(day, [span]);
  }

  return Array.from(byDay.entries())
    .sort(([a], [b]) => (a < b ? 1 : a > b ? -1 : 0))
    .map(([day, daySpans]) => ({
      day,
      spans: [...daySpans].sort((a, b) => Date.parse(b.from) - Date.parse(a.from)),
    }));
}

export type RangePreset = '7d' | '30d';

export interface ResolvedRange {
  /** ISO instant, inclusive. */
  from: string;
  /** ISO instant, inclusive. */
  to: string;
  /** `null` for an explicit custom range. */
  preset: RangePreset | null;
}

const PRESET_DAYS: Record<RangePreset, number> = { '7d': 7, '30d': 30 };

/** The page used to render nothing at all until the user picked two dates
 * — a blank screen with a disabled button. Presets now live in the URL as
 * `?range=7d`, so the no-parameters case is simply the 7-day preset and
 * needs no redirect, a shared link keeps meaning "the last 7 days" rather
 * than freezing an instant, and the picker can highlight the active preset
 * from the URL alone. Anything unparseable falls back to the default rather
 * than erroring: a mistyped query string should still show a useful page. */
export function resolveRange(
  params: { from?: string; to?: string; range?: string },
  now: number,
): ResolvedRange {
  const from = params.from ? Date.parse(params.from) : NaN;
  const to = params.to ? Date.parse(params.to) : NaN;
  if (!Number.isNaN(from) && !Number.isNaN(to) && from <= to) {
    return { from: params.from!, to: params.to!, preset: null };
  }

  const preset: RangePreset = params.range === '30d' ? '30d' : '7d';
  return {
    from: new Date(now - PRESET_DAYS[preset] * DAY_MS).toISOString(),
    to: new Date(now).toISOString(),
    preset,
  };
}
