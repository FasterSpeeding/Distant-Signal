import { LineChart } from '@mantine/charts';
import { Stack, Text, Title } from '@mantine/core';
import { getLineDailyStats } from '@/lib/api';
import type { LineDailyStats } from '@/lib/types';

// Placeholder, not a validated number -- see this plan's own "Open
// judgment calls" section and
// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md Open
// question 3. Revisit against real sample_cycles distributions once this
// has been running in production for a while.
const SPARSE_DATA_FLOOR_CYCLES = 20;

interface ChartPoint {
  day: string;
  delayRate: number | null;
  cancellationRate: number | null;
  skipRate: number | null;
  avgDelayMinutes: number | null;
  sampleCycles: number;
}

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
  const stats = await getLineDailyStats(id, from.slice(0, 10), to.slice(0, 10));

  if (stats.length === 0) {
    return <Text c="dimmed">Not enough sampled data yet for this line.</Text>;
  }

  const points = toChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Decision 7's honesty copy: rates are a share of sampled poll
          cycles, not a share of individual trains (Decision 2's
          cycle-weighted sampling) -- must not be softened or dropped. */}
      <Text size="sm" c="dimmed">
        Rates shown are the share of sampled poll cycles that looked delayed, cancelled, or skipping a stop --
        not a share of individual trains. Each point is based on that day&apos;s sample_cycles poll samples;
        days with too little coverage show as a gap rather than a misleading flat line.
      </Text>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Delay / cancellation / skip rate
        </Title>
        {/* The three rate metrics are 0-1 proportions and share one chart/axis.
            Average delay minutes is a different unit and is deliberately never
            combined onto this chart or its axis -- see the second LineChart
            below, and the plan's Global Constraints. */}
        <LineChart
          h={280}
          data={points}
          dataKey="day"
          series={[
            { name: 'delayRate', label: 'Delay rate', color: 'blue.6' },
            { name: 'cancellationRate', label: 'Cancellation rate', color: 'red.6' },
            { name: 'skipRate', label: 'Skip rate', color: 'yellow.6' },
          ]}
          valueFormatter={(value) => `${(value * 100).toFixed(1)}%`}
          connectNulls={false}
        />
      </Stack>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Average delay (minutes)
        </Title>
        <LineChart
          h={220}
          data={points}
          dataKey="day"
          series={[{ name: 'avgDelayMinutes', label: 'Avg delay (minutes)', color: 'grape.6' }]}
          connectNulls={false}
        />
      </Stack>
    </Stack>
  );
}
