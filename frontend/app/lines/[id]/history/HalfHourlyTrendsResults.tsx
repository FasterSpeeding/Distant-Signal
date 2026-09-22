import { Paper, Stack, Text } from '@mantine/core';
import { getLineHalfHourlyStats } from '@/lib/api';
import type { LineHalfHourlyStats } from '@/lib/types';
import { TrendsCharts } from './TrendsCharts';
import { HONESTY_COPY, HONESTY_COPY_DETAILS } from './TrendsResults';
import type { ChartPoint } from './chartPoint';

// Placeholder, not a validated number -- see
// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
// Decision 8 (written for the original 1-hour bucket). Deliberately NOT a
// reuse of TrendsResults.tsx's SPARSE_DATA_FLOOR_CYCLES (20 out of a
// ~1,440-cycle/day ceiling at the default 60s poll interval -- reusing it
// as-is against a 30-minute bucket's ~30-cycle ceiling would demand ~67%
// coverage, a far stricter bar than intended). This value is re-derived
// against the half-hourly ceiling, keeping Decision 8's own "roughly a
// third of the bucket's maximum possible coverage" ratio: a third of ~30
// is 10, half of the original hourly-era value of 20 (which was itself a
// third of the hourly ~60-cycle ceiling) -- halving the bucket duration
// halves the cycle ceiling, so the floor halves with it to hold the same
// ~33% coverage bar. Still an unvalidated placeholder -- revisit against
// real sample_cycles-per-bucket distributions once this has run in
// production.
const SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY = 10;

// Half-hourly sibling of TrendsResults.tsx's toChartPoints -- same
// null-all-four-fields-together gap logic, same connectNulls={false}
// rendering it feeds, different floor and a different source field
// (`halfHourStart`, an RFC3339 instant) becoming `bucketKey`.
export function toHalfHourlyChartPoints(stats: LineHalfHourlyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY;
    return {
      bucketKey: row.halfHourStart,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.sampleCycles,
    };
  });
}

/** Structurally parallel to `TrendsResults` (Decision 10) -- deliberately
 * a separate component, not a `granularity`-branching version of it,
 * since the fetch, sparse floor, and honesty copy are all genuinely
 * half-hourly-specific. The shared, reusable part is `TrendsCharts`
 * (generalized in the same plan's Task 9), which this renders exactly
 * the way `TrendsResults` does, passing `granularity="halfHour"`.
 * Originally `HourlyTrendsResults`, rendering 1-hour buckets -- renamed
 * (component, file, and every helper below) when the bucket size was
 * halved to 30 minutes; see git history for the hourly-era version. */
export async function HalfHourlyTrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  // This component is rendered inside a <Suspense> on /lines/[id]
  // (page.tsx). Suspense catches *suspension*, not errors -- an unhandled
  // rejection here propagates to app/error.tsx and replaces the entire
  // line page, so a backend outage would blank a page the rest of this
  // feature works to keep on screen. Deliberately NOT stale-served through
  // withStaleFallback: this is a secondary, decorative panel, and a
  // wrong-but-plausible trend chart is worse than an honest absence.
  //
  // Kept distinct from the "Not enough sampled data yet" branch below on
  // the same honesty grounds the copy in this file already observes: that
  // sentence is a claim about the *data*, and it would be a false one when
  // what actually happened is that we could not reach the service at all.
  let stats: LineHalfHourlyStats[];
  try {
    stats = await getLineHalfHourlyStats(id, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (stats.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet for this line.</Text>
      </Paper>
    );
  }

  const points = toHalfHourlyChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Was this file's own dense, jargon-heavy paragraph ("poll cycles",
          "coverage", `--` for a dash) -- exactly the leak the 2026-09-22 UX
          review ([OH] §3.3/I10) flagged and fixed on every OTHER
          Trends-page methodology explanation (TrendsResults.tsx's own
          honesty copy, and its NetworkTrendsResults.tsx/
          OperatorTrendsResults.tsx callers), just never carried over to
          this component. Reusing TrendsResults.tsx's HONESTY_COPY /
          HONESTY_COPY_DETAILS constants directly (this component is always
          granularity="halfHour", so no Record indirection is needed here)
          rather than re-deriving new half-hourly copy that could drift
          from theirs. */}
      <Text size="sm" c="dimmed">
        {HONESTY_COPY.halfHour}
      </Text>
      {/* order={3}: this sits under /lines/[id]'s h1 line name -> h2
          "Recent trends (last 24 hours)" -- h3 keeps the chart titles one
          level below that h2, with no skip. */}
      <TrendsCharts points={points} granularity="halfHour" order={3} showVolume />
      {/* Same collapsed-disclosure pattern as TrendsResults.tsx's own
          render -- data leads, the fuller mechanical explanation sits
          behind "How these rates are calculated" rather than printed in
          full every time. */}
      <details>
        <summary>How these rates are calculated</summary>
        <Text size="sm" c="dimmed" mt="xs">
          {HONESTY_COPY_DETAILS.halfHour}
        </Text>
      </details>
    </Stack>
  );
}
