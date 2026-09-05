import { Suspense } from 'react';
import { Alert, Divider, Skeleton, Stack, Tabs, TabsList, TabsPanel, TabsTab, Text, Title } from '@mantine/core';
import { getHistoryRetention, getLineStatus, getLineStatusHistory } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import {
  availableGranularities,
  granularityShortfallDays,
  groupHistoryByDay,
  resolveGranularity,
  resolveRange,
  retentionShortfallDays,
} from '@/lib/history';
import { formatDate, formatTime } from '@/lib/dateFormat';
import { GranularityControl } from './GranularityControl';
import { HistoryRangePicker } from './HistoryRangePicker';
import { TrendsResults } from './TrendsResults';
import { CoverageTrendsResults } from './CoverageTrendsResults';

// Same `revalidate = 0` rationale as the other dynamic routes: without it
// Next.js may treat this route as eligible for static generation and try to
// prerender it at build time, which fails since the `api` service only
// exists on the compose network at runtime. Previously this page was
// implicitly dynamic because it rendered nothing without `searchParams`;
// now that it has a default range, say so explicitly.
export const revalidate = 0;

// Same lookup the detail page uses for its heading. This page's job is the
// date range, not the name, so a failed lookup (line deleted, catalogue
// hiccup, etc.) falls back to the raw id instead of taking the whole page
// down the way `notFound()` would on the detail page. An empty result is
// the same fallback rather than a thrown TypeError, and the failure is
// logged so a genuine bug in here doesn't vanish silently. Mirrors
// `resolveStationName` on the station page.
async function resolveLineName(id: string): Promise<string> {
  try {
    const [report] = await getLineStatus([id], false);
    return report?.name ?? id;
  } catch (err) {
    console.warn(`Could not resolve a name for line "${id}"; falling back to the id.`, err);
    return id;
  }
}

/** The three real retention ceilings the Timeline/Trends tabs need, or
 * safe fallbacks if the fetch fails. `historyRetentionDays` keeps its
 * existing `null`-means-unknown/hide-the-banner semantics for the Timeline
 * tab (unchanged -- Non-goal of
 * docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md).
 * The two new fields (Decision 8) default to `0` on failure rather than
 * `null`: `resolveGranularity`/`availableGranularities` need concrete
 * numbers, and `0` (combined with `'day'` always being exempt from both
 * checks -- see `frontend/lib/history.ts`) collapses safely to "only Daily
 * is offered" -- the same "don't guess, degrade to the least you can
 * promise" posture the Timeline banner already takes, extended one step
 * further here (hide the choice too, not just the notice). */
async function resolveRetention(): Promise<{
  historyRetentionDays: number | null;
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}> {
  try {
    const retention = await getHistoryRetention();
    return {
      historyRetentionDays: retention.historyRetentionDays,
      dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
      halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
    };
  } catch (err) {
    console.warn('Could not resolve retention ceilings; hiding the retention notice and offering only Daily.', err);
    return { historyRetentionDays: null, dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 };
  }
}

export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string; granularity?: string }>;
}) {
  const { id } = await params;
  const query = await searchParams;

  const now = Date.now();
  const [name, retention] = await Promise.all([
    resolveLineName(id),
    resolveRetention(),
  ]);
  const range = resolveRange(query, now);
  const shortfallDays = retentionShortfallDays(range, retention.historyRetentionDays, now);

  const rangeWidthMs = Date.parse(range.to) - Date.parse(range.from);
  const ceilings = {
    dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
    halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
  };
  const available = availableGranularities(rangeWidthMs, ceilings);
  const granularity = resolveGranularity(query, rangeWidthMs, ceilings);
  const granularityShortfall = granularityShortfallDays(range, granularity, ceilings, now);
  const retentionDaysForGranularity =
    granularity === 'day' ? retention.dailyStatsRetentionDays : Math.floor(retention.halfHourlyStatsRetentionHours / 24);

  return (
    <Stack p="lg" gap="md">
      <TextLink href={`/lines/${id}`} underline="always">
        Back to line
      </TextLink>
      <Title order={1}>History: {name}</Title>
      <HistoryRangePicker lineId={id} preset={range.preset} from={range.from} to={range.to} />
      {/* Timeline (the existing per-status-change history) and Trends (the
          Task 9 daily rollup) are split into tabs since they're different
          views over data with different retention windows — the 7-day
          shortfall notice below is specific to `line_status_history` and
          would be misleading if shown while looking at Trends, so it lives
          inside the Timeline panel rather than above the Tabs.

          `TabsList`/`TabsTab`/`TabsPanel` -- the flat named exports, not
          the `Tabs.List`/`Tabs.Tab`/`Tabs.Panel` dot-notation compound API
          -- for the same reason `AllLinesTable.tsx` uses `TableThead`/
          `TableTr`/etc instead of `Table.Thead`: this page is a Server
          Component (`Tabs` itself carries a `"use client"` directive), and
          a dot-notation sub-component accessed off a Client Component's
          reference the RSC boundary hands to a Server Component resolves
          to `undefined` at render time -- confirmed live against a running
          dev server: this route 500'd with "Element type is invalid ...
          got: undefined" until this was switched to the flat imports. Not
          merely a lint nit; this was the actual "history page doesn't
          work" bug. */}
      <Tabs defaultValue="timeline">
        <TabsList>
          <TabsTab value="timeline">Timeline</TabsTab>
          <TabsTab value="trends">Trends</TabsTab>
        </TabsList>
        <TabsPanel value="timeline">
          <Stack gap="md" pt="md">
            {/* Distinguishes "nothing happened in this window" from "the window
                reaches further back than this server keeps history" — without
                this, a mostly-empty "Last 30 days" result and three genuinely
                quiet weeks look identical. `shortfallDays` (and therefore this
                banner) is only non-null when the real, server-reported retention
                ceiling is known AND the requested range exceeds it — never a
                guess. See `lib/history.ts`'s `retentionShortfallDays`. */}
            {shortfallDays !== null && (
              <Alert color="yellow" variant="light" title="Some of this range isn't available">
                This server only keeps {retention.historyRetentionDays}{' '}
                {retention.historyRetentionDays === 1 ? 'day' : 'days'} of line
                history. The oldest {shortfallDays} {shortfallDays === 1 ? 'day' : 'days'} of the range you
                picked has already been removed — if this range looks empty or short, that may be why,
                not because nothing happened.
              </Alert>
            )}
            {/* The results are always rendered now, so without a Suspense
                boundary the whole page — picker included — would block on the
                history fetch, which is the slowest call in the app for a 30-day
                window.

                Keyed on the preset name when one is active, not on `from`/`to`:
                `resolveRange` stamps a preset's `from`/`to` from `Date.now()` at
                millisecond precision, and `AutoRefresh` re-renders this page every
                30s, so an `${from}-${to}` key would churn — and remount this whole
                subtree — on every auto-refresh even though the user is still
                looking at "the last 7 days". A preset's *identity* is its name,
                not the instant it happened to be computed at. A genuine custom
                range has no preset (`range.preset` is `null`), so it still falls
                back to the from/to-based key and resets exactly as before. */}
            <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={240} />}>
              <HistoryResults id={id} from={range.from} to={range.to} />
            </Suspense>
          </Stack>
        </TabsPanel>
        <TabsPanel value="trends">
          <Stack gap="md" pt="md">
            <GranularityControl
              lineId={id}
              preset={range.preset}
              from={range.from}
              to={range.to}
              granularity={granularity}
              available={available}
            />
            {/* Sub-daily-aware sibling of the Timeline tab's own shortfall
                banner (Decision 8) -- only non-null when the CURRENTLY
                SELECTED tier's own real retention ceiling doesn't reach
                back to range.from. A custom range can still outrun a
                35-day sub-daily retention or a 300-day daily one even
                though GranularityControl already hides tiers that can't
                cover the FULL range -- this covers the case where the
                selected tier partially, not fully, exceeds its ceiling. */}
            {granularityShortfall !== null && (
              <Alert color="yellow" variant="light" title="Some of this range isn't available at this granularity">
                This server only keeps {retentionDaysForGranularity}{' '}
                {retentionDaysForGranularity === 1 ? 'day' : 'days'} of data at this granularity. The oldest{' '}
                {granularityShortfall} {granularityShortfall === 1 ? 'day' : 'days'} of the range you picked has
                already been removed — if this range looks empty or short, that may be why, not because nothing
                happened.
              </Alert>
            )}
            <Suspense
              key={`${granularity}-${range.preset ?? `${range.from}-${range.to}`}`}
              fallback={<Skeleton height={320} />}
            >
              <TrendsResults id={id} from={range.from} to={range.to} granularity={granularity} />
            </Suspense>
            {/* Decision 4's daily full-coverage series -- a second,
                separate section under the existing sample-based one, always
                attempted (not conditionally hidden behind a pre-check for
                whether this line has any coverage rows). Judgment call: the
                simplest of the three UI shapes the design doc's Open
                Question 4 leaves open (separate series vs. one series with
                a marked transition vs. hiding the sample series once
                coverage exists) -- chosen because it needs no new
                "does this line have coverage data at all" lookup and
                degrades to an honest, harmless empty-state message today,
                since nothing produces full-coverage data yet. */}
            <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={320} />}>
              <CoverageTrendsResults id={id} from={range.from} to={range.to} />
            </Suspense>
          </Stack>
        </TabsPanel>
      </Tabs>
    </Stack>
  );
}

// Exported (unlike an ordinary route-local helper) purely for
// testability, the same reason TrendsResults/HalfHourlyTrendsResults are
// their own modules: this page's <Suspense><HistoryResults .../></Suspense>
// wraps an async Server Component, and this repo's jsdom/@testing-library
// harness has no RSC runtime to resolve that promise -- rendering
// LineHistoryPage and waiting on the Timeline tab's Suspense boundary to
// settle hangs on the Skeleton fallback forever (confirmed: no existing
// test in this file asserts on HistoryResults' resolved content). Awaiting
// this function directly, the same way TrendsResults.test.tsx does for its
// sibling, sidesteps the Suspense boundary entirely.
export async function HistoryResults({ id, from, to }: { id: string; from: string; to: string }) {
  const entries = await getLineStatusHistory(id, from, to);
  const days = groupHistoryByDay(entries);
  const spanCount = days.reduce((total, day) => total + day.spans.length, 0);

  if (days.length === 0) {
    return <Text c="dimmed">No history entries in that range.</Text>;
  }

  return (
    <Stack gap="lg">
      {/* Says out loud what the grouping did, so a short page doesn't read
          as missing data. */}
      <Text size="sm" c="dimmed">
        {entries.length} status {entries.length === 1 ? 'recompute' : 'recomputes'} across {spanCount}{' '}
        {spanCount === 1 ? 'incident' : 'incidents'}, newest first.
      </Text>
      {days.map((day) => (
        <Stack key={day.day} gap="xs">
          {/* order={2}, not 3: this page's only other heading is the
              `History: {name}` h1 at :74 -- there is no h2 between them,
              so an h3 here skipped a level (axe `heading-order`).
              `size="h5"` is unchanged, so this is a tag-only change with
              no visual effect. Both TabsPanels are mounted at once
              (Mantine Tabs keepMounted defaults to true), so this and the
              Trends tab's chart headings both have to land at h2 for the
              document to be skip-free either way the tabs are read. */}
          <Title order={2} size="h5">
            {formatDate(day.spans[0].to)}
          </Title>
          <Divider />
          {day.spans.map((span) => (
            <div className="issueRow" key={`${span.reason}-${span.from}`}>
              <div className="issueRow__main">
                <div className="issueRow__badge">
                  <StatusBadge severity={span.severity} />
                </div>
                <Text size="sm" className="issueRow__reason">
                  {span.reason || 'No reason given'}
                  {span.flips.length > 1 && (
                    <Text span size="xs" c="dimmed">
                      {' '}
                      (severity changed {span.flips.length - 1} {span.flips.length - 1 === 1 ? 'time' : 'times'})
                    </Text>
                  )}
                </Text>
              </div>
              <div className="issueRow__meta">
                <Text size="xs" c="dimmed">
                  {span.from === span.to
                    ? formatTime(span.from)
                    : `${formatTime(span.from)}–${formatTime(span.to)}`}
                </Text>
              </div>
            </div>
          ))}
        </Stack>
      ))}
    </Stack>
  );
}
