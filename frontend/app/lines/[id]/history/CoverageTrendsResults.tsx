import { Paper, Stack, Text, Title } from '@mantine/core';
import { getLineDailyCoverageStats } from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { LineDailyCoverageStats } from '@/lib/types';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholder, not a validated number -- mirrors TrendsResults.tsx's own
// SPARSE_DATA_FLOOR_CYCLES, but deliberately its OWN constant, not a
// shared one: the design doc's Decision 4 explicitly leaves this
// uncalibrated ("not designed to a specific number here"), and the two
// floors are calibrated against different underlying cadences (LDBWS poll
// coverage vs. a future full-coverage consumer's own resolution cadence,
// which doesn't exist yet to calibrate against). Revisit once a real
// producer's resolved_windows distribution exists to look at.
const SPARSE_DATA_FLOOR_WINDOWS = 20;

/** Full-coverage sibling of `TrendsResults.tsx`'s own `toChartPoints` --
 * same sparse-gap-as-null shape, `resolvedWindows` in place of
 * `sampleCycles` as the coverage/gap signal. `ChartPoint.sampleCycles`
 * itself is reused rather than renamed here (it isn't read by
 * `TrendsCharts.tsx` for rendering, only threaded through for sparse-floor
 * test assertions -- see that type's own doc comment), so this data still
 * flows through the exact same, unmodified `TrendsCharts` component. */
export function toCoverageChartPoints(stats: LineDailyCoverageStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.resolvedWindows < SPARSE_DATA_FLOOR_WINDOWS;
    return {
      bucketKey: row.day,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      sampleCycles: row.resolvedWindows,
    };
  });
}

/** Decision 4's daily full-coverage series -- a second, separate chart
 * section rendered alongside (not replacing) `TrendsResults`' existing
 * sample-derived one. Always resolves an empty array today, since no
 * full-coverage producer exists yet to populate
 * `line_status_daily_coverage_stats` -- renders the honest "not enough
 * data yet" fallback in that case, same posture `TrendsResults` itself
 * takes for a genuinely quiet line.
 *
 * Only the DAILY coverage series gets a chart in this pass -- the backend
 * half-hourly route/table exist (for symmetry with the existing
 * daily/half-hourly pair, and so a future producer has both available
 * immediately), but a second, definitely-still-empty half-hourly chart
 * surface adds nothing a viewer can act on today. See this repo's
 * 2026-09-03 full-coverage-metrics-scaffolding plan's own Non-goals. */
export async function CoverageTrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  const stats = await getLineDailyCoverageStats(id, londonDayKey(from), londonDayKey(to));

  if (stats.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough full-coverage data yet for this line.</Text>
      </Paper>
    );
  }

  const points = toCoverageChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Honesty copy for the full-coverage rollup -- deliberately NOT the
          sample-rollup copy with "trains" swapped for "services": the
          population-coverage gap the sample copy hedges against doesn't
          exist here (every scheduled service is in view by construction),
          so this copy states that plainly instead of carrying over a hedge
          that no longer applies. See
          docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
          Decision 4. */}
      <Text size="sm" c="dimmed">
        Rates shown cover every scheduled service on this line, cross-referenced against real train-movement data —
        not a sample of live departures at a handful of stations.
      </Text>
      <Title order={3} size="h6">
        Full coverage
      </Title>
      <TrendsCharts points={points} granularity="day" order={4} />
    </Stack>
  );
}
