import { Suspense } from 'react';
import { Alert, Divider, Skeleton, Stack, Tabs, TabsList, TabsPanel, TabsTab, Text, Title } from '@mantine/core';
import { getHistoryRetention, getLineStatus, getLineStatusHistory } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import { groupHistoryByDay, resolveRange, retentionShortfallDays } from '@/lib/history';
import { formatDate, formatTime } from '@/lib/dateFormat';
import { HistoryRangePicker } from './HistoryRangePicker';
import { TrendsResults } from './TrendsResults';

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

/** The real `line_status_history` retention ceiling, or `null` if it
 * couldn't be fetched. `null` means "unknown" — `retentionShortfallDays`
 * treats that as "nothing to warn about" rather than guessing a number,
 * same fallback posture as `resolveLineName` above (degrade quietly rather
 * than take the whole page down over a non-essential value). */
async function resolveHistoryRetentionDays(): Promise<number | null> {
  try {
    const { historyRetentionDays } = await getHistoryRetention();
    return historyRetentionDays;
  } catch (err) {
    console.warn('Could not resolve the history retention window; hiding the retention notice.', err);
    return null;
  }
}

export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string }>;
}) {
  const { id } = await params;
  const query = await searchParams;

  const now = Date.now();
  const [name, retentionDays] = await Promise.all([
    resolveLineName(id),
    resolveHistoryRetentionDays(),
  ]);
  const range = resolveRange(query, now);
  const shortfallDays = retentionShortfallDays(range, retentionDays, now);

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
                This server only keeps {retentionDays} {retentionDays === 1 ? 'day' : 'days'} of line
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
            <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={320} />}>
              <TrendsResults id={id} from={range.from} to={range.to} />
            </Suspense>
          </Stack>
        </TabsPanel>
      </Tabs>
    </Stack>
  );
}

async function HistoryResults({ id, from, to }: { id: string; from: string; to: string }) {
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
          <Title order={3} size="h5">
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
