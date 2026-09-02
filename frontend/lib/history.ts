import { londonDayKey } from './dateFormat';
import { severityRank } from './severity';
import type { LineStatus, LineStatusHistoryEntry } from './types';

const DAY_MS = 86_400_000;

const LIVE_SAMPLE_ANNOTATION = / \(live samples show: [^)]*\)$/;

/** Strips the `" (live samples show: ...)"` annotation `escalate_from_sample_stats`
 * (crates/aggregator/src/aggregation.rs) appends to `reason` on escalation.
 * Its counts roll over almost every poll cycle even when the underlying
 * incident hasn't changed, so it must not participate in incident identity
 * — mirrors `strip_live_sample_annotation` in crates/aggregator/src/queries.rs,
 * which strips the same suffix before the aggregator decides whether to
 * write a new history row at all. */
function coreReason(reason: string): string {
  return reason.replace(LIVE_SAMPLE_ANNOTATION, '');
}

/** Stand-in for "this recompute had no active status" (i.e. good service),
 * so quiet periods group into their own identity the same way a named
 * incident does, instead of vanishing from the timeline entirely. */
const NO_ACTIVE_STATUS: LineStatus = {
  statusSeverity: 10,
  statusSeverityDescription: 'Good Service',
  reason: '',
  dataQuality: 'knowledgebase',
  validityPeriods: [],
  sampleAvailability: { state: 'no-coverage' },
};

export interface SeverityFlip {
  /** True severity rank order, not raw `statusSeverity` — see `severityRank`. */
  severity: number;
  /** A representative status at this severity, for rendering detail. */
  status: LineStatus;
  /** `computedAt` of the first recompute in this run. */
  from: string;
  /** `computedAt` of the last recompute in this run. */
  to: string;
  /** How many recomputes were collapsed into this run. */
  samples: number;
}

export interface HistorySpan {
  /** Incident identity: `reason` with the live-sample annotation stripped.
   * Empty string means "no active status" (good service). */
  reason: string;
  /** Worst severity (by true rank) hit anywhere in this span. */
  severity: number;
  /** The status instance at that worst severity, for rendering full detail
   * (disruption, validity periods, etc). */
  status: LineStatus;
  /** First time this incident was seen within its day. */
  from: string;
  /** Last time this incident was seen within its day. */
  to: string;
  /** How many recomputes this span rolls up. */
  samples: number;
  /** Consecutive same-severity runs within the span, chronological order.
   * Length 1 means the incident never changed severity. */
  flips: SeverityFlip[];
}

export interface HistoryDay {
  /** `YYYY-MM-DD`, London. */
  day: string;
  spans: HistorySpan[];
}

function worstOf(a: LineStatus, b: LineStatus): LineStatus {
  return severityRank(b.statusSeverity) > severityRank(a.statusSeverity) ? b : a;
}

/** Groups one day's recomputes into one row per distinct ongoing incident
 * (identified by `reason` with live-sample-count noise stripped, ignoring
 * severity), regardless of whether its occurrences are contiguous in time.
 * An incident whose severity flaps in and out — e.g. live-sample escalation
 * repeatedly crossing a threshold — reads as one row spanning its full
 * first-to-last-seen window in the day, instead of fragmenting into as many
 * rows as it flapped, scattered in whatever order the flaps happened to
 * interleave with other incidents. A synthetic "no active status" identity
 * groups the line's good-service gaps the same way. */
function collapseDay(entries: LineStatusHistoryEntry[]): HistorySpan[] {
  const ordered = [...entries].sort((a, b) => Date.parse(a.computedAt) - Date.parse(b.computedAt));

  const byIdentity = new Map<string, { at: string; status: LineStatus }[]>();
  for (const entry of ordered) {
    const statuses = entry.lineStatuses.length > 0 ? entry.lineStatuses : [NO_ACTIVE_STATUS];
    for (const status of statuses) {
      const key = coreReason(status.reason);
      const points = byIdentity.get(key);
      if (points) points.push({ at: entry.computedAt, status });
      else byIdentity.set(key, [{ at: entry.computedAt, status }]);
    }
  }

  const spans: HistorySpan[] = [];
  for (const [reason, points] of byIdentity) {
    const flips: SeverityFlip[] = [];
    for (const point of points) {
      const current = flips[flips.length - 1];
      if (current && current.severity === point.status.statusSeverity) {
        current.to = point.at;
        current.samples += 1;
        continue;
      }
      flips.push({ severity: point.status.statusSeverity, status: point.status, from: point.at, to: point.at, samples: 1 });
    }

    const worst = points.reduce((worst, point) => worstOf(worst, point.status), points[0].status);

    spans.push({
      reason,
      severity: worst.statusSeverity,
      status: worst,
      from: points[0].at,
      to: points[points.length - 1].at,
      samples: points.length,
      flips,
    });
  }

  return spans;
}

/** The aggregator recomputes every 5–15 minutes, so a 30-day history is
 * thousands of entries describing a handful of actual incidents. Entries
 * are bucketed into London calendar days first — so a summer evening
 * doesn't split across two headings at 23:00 UTC, and a long-flapping
 * incident's grouping stays bounded to one day's worth of recomputes —
 * then each day's entries are collapsed by `collapseDay`. Days are
 * newest-first; spans within a day are newest-first by their last
 * occurrence. */
export function groupHistoryByDay(entries: LineStatusHistoryEntry[]): HistoryDay[] {
  const byDay = new Map<string, LineStatusHistoryEntry[]>();
  for (const entry of entries) {
    const day = londonDayKey(entry.computedAt);
    const existing = byDay.get(day);
    if (existing) existing.push(entry);
    else byDay.set(day, [entry]);
  }

  return Array.from(byDay.entries())
    .sort(([a], [b]) => (a < b ? 1 : a > b ? -1 : 0))
    .map(([day, dayEntries]) => ({
      day,
      spans: collapseDay(dayEntries).sort((a, b) => Date.parse(b.to) - Date.parse(a.to)),
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

const HOUR_MS = 3_600_000;

/** The line-info-page embed's fixed rolling 24-hour window -- deliberately
 * NOT a `RangePreset`/`resolveRange` variant: Decision 11 of
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
 * is explicit that this view has no user-selectable range at all, unlike
 * the history page's day/30-day presets (which live in the URL). No
 * `preset`/`from`/`to`-from-query-params handling is needed here for the
 * same reason -- this always resolves the same window relative to `now`.
 * The window itself (24 hours) is independent of the chart's bucket
 * granularity, so this logic is unchanged from its original
 * `resolveHourlyRange` form -- only the name changed, to stay consistent
 * with the rest of this feature's rename when the bucket size was halved
 * to 30 minutes (see `HalfHourlyTrendsResults.tsx`, which is this
 * function's only caller). */
export function resolveHalfHourlyRange(now: number): { from: string; to: string } {
  return {
    from: new Date(now - 24 * HOUR_MS).toISOString(),
    to: new Date(now).toISOString(),
  };
}

/** Whether `range.from` reaches back further than the backend's real
 * `line_status_history` retention window allows — and if so, by how many
 * whole days. `retentionDays` is the real, server-reported ceiling (see
 * `lib/api.ts`'s `getHistoryRetention`), never a guessed/hardcoded number:
 * it's an admin-configurable knob (`crates/aggregator/src/config.rs`'s
 * `history_retention_days`, default 7) that can legitimately differ across
 * deployments, so this must be checked against the real value, not assumed.
 *
 * Returns `null` when the requested range is fully within what's retained
 * (nothing to warn about) or when `retentionDays` is unknown (the caller
 * couldn't fetch it — see `resolveHistoryRetentionDays` in `page.tsx`, which
 * degrades to `null` on fetch failure rather than guessing).
 *
 * This exists because a truncated result and a genuinely quiet line are
 * otherwise indistinguishable to a user: `resolveRange`'s "Last 30 days"
 * preset can ask for a window wider than what the backend has ever kept,
 * and the history route just returns whatever rows still exist in range —
 * silently fewer than requested, with no signal that the rest was pruned
 * rather than simply uneventful. */
export function retentionShortfallDays(
  range: Pick<ResolvedRange, 'from'>,
  retentionDays: number | null,
  now: number,
): number | null {
  if (retentionDays === null) return null;
  const fromMs = Date.parse(range.from);
  if (Number.isNaN(fromMs)) return null;
  const oldestRetainedMs = now - retentionDays * DAY_MS;
  if (fromMs >= oldestRetainedMs) return null;
  return Math.ceil((oldestRetainedMs - fromMs) / DAY_MS);
}
