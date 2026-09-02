import { Paper, Stack, Text } from '@mantine/core';
import { getLineHourlyStats } from '@/lib/api';
import type { LineHourlyStats } from '@/lib/types';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholder, not a validated number -- see
// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
// Decision 8. Deliberately NOT a reuse of TrendsResults.tsx's
// SPARSE_DATA_FLOOR_CYCLES (20 out of a ~1,440-cycle/day ceiling at the
// default 60s poll interval -- reusing it as-is against an hour's
// ~60-cycle ceiling would demand ~33% coverage, a much stricter bar).
// This value (also 20, but re-derived against the hourly ceiling as
// roughly a third of an hour's maximum possible coverage) is Decision 8's
// own "more defensible starting placeholder" -- revisit against real
// sample_cycles-per-hour distributions once this has run in production.
const SPARSE_DATA_FLOOR_CYCLES_HOURLY = 20;

// Hourly sibling of TrendsResults.tsx's toChartPoints -- same
// null-all-four-fields-together gap logic, same connectNulls={false}
// rendering it feeds, different floor and a different source field
// (`hourStart`, an RFC3339 instant) becoming `bucketKey`.
export function toHourlyChartPoints(stats: LineHourlyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES_HOURLY;
    return {
      bucketKey: row.hourStart,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      sampleCycles: row.sampleCycles,
    };
  });
}

/** Structurally parallel to `TrendsResults` (Decision 10) -- deliberately
 * a separate component, not a `granularity`-branching version of it,
 * since the fetch, sparse floor, and honesty copy are all genuinely
 * hourly-specific. The shared, reusable part is `TrendsCharts`
 * (generalized in the same plan's Task 9), which this renders exactly
 * the way `TrendsResults` does, passing `granularity="hour"`. */
export async function HourlyTrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  const stats = await getLineHourlyStats(id, from, to);

  if (stats.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet for this line.</Text>
      </Paper>
    );
  }

  const points = toHourlyChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Same honesty-copy posture as TrendsResults.tsx's own comment
          (marked "Must not be softened or dropped") -- reworded from "that
          day" to "that hour" per Decision 2's hourly attribution, not a
          new tradeoff. */}
      <Text size="sm" c="dimmed">
        Rates shown count each distinct train once per hour, based on its status the first time it was seen that
        hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later while
        still in view will still show here as on time. Hours with too little coverage show as a gap rather than a
        misleading flat line.
      </Text>
      <TrendsCharts points={points} granularity="hour" />
    </Stack>
  );
}
