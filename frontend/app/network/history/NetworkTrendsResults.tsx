import { Paper, Stack, Text } from '@mantine/core';
import {
  getNetworkDailyStats,
  getNetworkHalfHourlyStats,
  getNetworkHourlyStats,
  getNetworkSixHourlyStats,
} from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { HONESTY_COPY, HONESTY_COPY_DETAILS, SPARSE_FLOOR, toChartPoints } from '@/app/lines/[id]/history/TrendsResults';
import { TrendsCharts } from '@/app/lines/[id]/history/TrendsCharts';
import type { ChartPoint } from '@/app/lines/[id]/history/chartPoint';

// Dispatches to the right fetch + floor + bucket-key field for the
// selected tier -- network-scoped sibling of TrendsResults.tsx's own
// fetchPoints (and OperatorTrendsResults.tsx's), reusing its
// SPARSE_FLOOR/toChartPoints (Judgment Call 3 of
// docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md)
// rather than re-deriving them.
async function fetchPoints(granularity: TrendGranularity, from: string, to: string): Promise<ChartPoint[]> {
  switch (granularity) {
    case 'day': {
      const stats = await getNetworkDailyStats(londonDayKey(from), londonDayKey(to));
      return toChartPoints(stats, (row) => row.day, SPARSE_FLOOR.day);
    }
    case 'halfHour': {
      const stats = await getNetworkHalfHourlyStats(from, to);
      return toChartPoints(stats, (row) => row.halfHourStart, SPARSE_FLOOR.halfHour);
    }
    case 'hour': {
      const stats = await getNetworkHourlyStats(from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.hour);
    }
    case 'sixHour': {
      const stats = await getNetworkSixHourlyStats(from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.sixHour);
    }
  }
}

/** Network-scoped sibling of `TrendsResults`/`OperatorTrendsResults` --
 * every catalogue (National Rail) line, summed. See this plan's Judgment
 * Call 5: TfL lines never contribute (they carry no rows in the
 * underlying rollup tables at all), so this is honestly a National Rail
 * network view, not literally "every mode." */
export async function NetworkTrendsResults({
  from,
  to,
  granularity = 'day',
}: {
  from: string;
  to: string;
  granularity?: TrendGranularity;
}) {
  let points: ChartPoint[];
  try {
    points = await fetchPoints(granularity, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (points.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet across the network.</Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      {/* Template literal, not `{expr} text…` split across the expression
          boundary -- see OperatorTrendsResults.tsx's identical comment for
          why (the same fix for the "running.Rates" finding, [OH] §3.3). */}
      <Text size="sm" c="dimmed">
        {`${HONESTY_COPY[granularity]} Rates shown are summed across every National Rail line this app tracks. TfL isn't included yet.`}
      </Text>
      <TrendsCharts points={points} granularity={granularity} order={2} showVolume />
      <details>
        <summary>How these rates are calculated</summary>
        <Text size="sm" c="dimmed" mt="xs">
          {HONESTY_COPY_DETAILS[granularity]}
        </Text>
      </details>
    </Stack>
  );
}
