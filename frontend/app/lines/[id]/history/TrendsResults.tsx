import { Stack, Text, Title } from '@mantine/core';
import { getLineDailyStats } from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { LineDailyStats } from '@/lib/types';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholder, not a validated number -- see this plan's own "Open
// judgment calls" section and
// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md Open
// question 3. Revisit against real sample_cycles distributions once this
// has been running in production for a while.
const SPARSE_DATA_FLOOR_CYCLES = 20;

// Turns a day with too little poll coverage into a gap rather than a
// misleading flat/zero line -- Decision 3 of
// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md. Recharts
// (which @mantine/charts' LineChart wraps) renders a genuine break in the
// line across a `null` data point when `connectNulls={false}` is set on the
// chart, rather than interpolating across it -- verified against the real,
// installed `@mantine/charts`/`recharts` types and the live Mantine docs,
// not assumed; see this task's report for the specifics.
export function toChartPoints(stats: LineDailyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES;
    return {
      day: row.day,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      sampleCycles: row.sampleCycles,
    };
  });
}

export async function TrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  const stats = await getLineDailyStats(id, londonDayKey(from), londonDayKey(to));

  if (stats.length === 0) {
    return <Text c="dimmed">Not enough sampled data yet for this line.</Text>;
  }

  const points = toChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Honesty copy for the dedup-driven daily rollup (Ruling A,
          .superpowers/sdd/2026-08-31-line-history-graphics/progress.md):
          record_daily_stats now sums dedup::dedup_new_sample_stats's DEDUPED
          output, so each distinct train (by Darwin service_id) is counted
          once per day, not once per poll cycle it stayed visible. But
          SeenServiceLedger.mark_seen classifies a train by whichever cycle
          FIRST observed it -- a train seen on-time first and delayed later
          in the same visit is never re-classified. This is a deliberate
          accuracy tradeoff against a persisted "last known state" ledger,
          which would need a bigger redesign than this rollup -- not
          something to build here. Must not be softened or dropped. */}
      <Text size="sm" c="dimmed">
        Rates shown count each distinct train once per day, based on its status the first time it was seen that
        day -- not a share of poll cycles. A train that starts on time and only becomes delayed later while
        still in view will still show here as on time. Days with too little coverage show as a gap rather than a
        misleading flat line.
      </Text>
      {/* Both charts (including their `valueFormatter`) live in this
          Client Component -- see its own doc comment for why: a plain
          function prop like `valueFormatter` can't cross the Server-to-
          Client boundary straight out of this `async` Server Component. */}
      <TrendsCharts points={points} />
    </Stack>
  );
}
