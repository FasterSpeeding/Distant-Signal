import { Paper, Stack, Text, Title } from '@mantine/core';
import { getLineHalfHourlyCoverageStats } from '@/lib/api';
import type { LineHalfHourlyCoverageStats } from '@/lib/types';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholder, not a validated number -- half-hourly sibling of
// CoverageTrendsResults.tsx's own SPARSE_DATA_FLOOR_WINDOWS (20), halved
// following the exact precedent HalfHourlyTrendsResults.tsx's own
// SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY comment already established:
// halving the bucket duration halves the cycle/window ceiling, so the
// floor halves with it to hold the same ~33% coverage bar. Doubly
// unvalidated -- it is a halving of an already-unvalidated daily
// placeholder, and there is still no real full-coverage producer to
// observe a resolved_windows-per-half-hour distribution from. See
// docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md
// Decision 2 / Open question 1.
const SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY = 10;

/** Half-hourly sibling of `CoverageTrendsResults.tsx`'s own
 * `toCoverageChartPoints` -- same sparse-gap-as-null shape,
 * `resolvedWindows` as the coverage/gap signal, `halfHourStart` (an
 * RFC3339 instant) as `bucketKey`, the same shape
 * `toHalfHourlyChartPoints` already uses for its own `bucketKey`. */
export function toHalfHourlyCoverageChartPoints(stats: LineHalfHourlyCoverageStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.resolvedWindows < SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY;
    return {
      bucketKey: row.halfHourStart,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.resolvedWindows,
    };
  });
}

/** Half-hourly full-coverage Trends chart -- structurally parallel to
 * `HalfHourlyTrendsResults.tsx` (the sample-derived half-hourly chart) the
 * same way `CoverageTrendsResults.tsx` is structurally parallel to
 * `TrendsResults.tsx`. Deliberately a separate component, not a
 * `granularity`-branching version of `CoverageTrendsResults`, for the same
 * reason the sample-series pair is split (granularity design Decision
 * 10): the fetch, sparse floor, and honesty copy are all
 * granularity-specific content, not shared plumbing. The shared, reusable
 * part is `TrendsCharts`, rendered here exactly the way
 * `CoverageTrendsResults` renders it, passing `granularity="halfHour"`.
 * See docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md
 * Decision 1. */
export async function HalfHourlyCoverageTrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  // Same unreachable-backend posture as HalfHourlyTrendsResults.tsx: this
  // component is rendered inside its own <Suspense> on /lines/[id]
  // (page.tsx), which catches suspension but not errors -- an unhandled
  // rejection here would propagate to app/error.tsx and blank the whole
  // page. Deliberately NOT stale-served: this is a secondary, decorative
  // panel, and a wrong-but-plausible trend chart is worse than an honest
  // absence.
  let stats: LineHalfHourlyCoverageStats[];
  try {
    stats = await getLineHalfHourlyCoverageStats(id, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Coverage trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (stats.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough full-coverage data yet for this line.</Text>
      </Paper>
    );
  }

  const points = toHalfHourlyCoverageChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Reused verbatim from CoverageTrendsResults.tsx, no rewording --
          unlike the sample-series pair's copy (which reworded "that day"
          to "that half hour" for its per-bucket attribution rule), this
          sentence states a population fact (every scheduled service is
          cross-referenced, by construction, regardless of bucket size),
          not a per-bucket counting rule -- so there is nothing here for a
          granularity change to reword. See design doc Decision 3. */}
      <Text size="sm" c="dimmed">
        Rates shown cover every scheduled service on this line, cross-referenced against real train-movement data —
        not a sample of live departures at a handful of stations.
      </Text>
      {/* order={3}: this sits under /lines/[id]'s h1 line name -> h2
          "Recent trends (last 24 hours)" -> HalfHourlyTrendsResults' own
          two h3 chart titles -- this section's own h3 keeps the same
          level, no skip. */}
      <Title order={3} size="h6">
        Full coverage
      </Title>
      <TrendsCharts points={points} granularity="halfHour" order={4} />
    </Stack>
  );
}
