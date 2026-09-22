import { Paper, Stack, Text } from '@mantine/core';
import {
  getOperatorDailyStats,
  getOperatorHalfHourlyStats,
  getOperatorHourlyStats,
  getOperatorSixHourlyStats,
} from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { HONESTY_COPY, SPARSE_FLOOR, toChartPoints } from '@/app/lines/[id]/history/TrendsResults';
import { TrendsCharts } from '@/app/lines/[id]/history/TrendsCharts';
import type { ChartPoint } from '@/app/lines/[id]/history/chartPoint';

// Dispatches to the right fetch + floor + bucket-key field for the
// selected tier -- operator-scoped sibling of TrendsResults.tsx's own
// fetchPoints, reusing its SPARSE_FLOOR/toChartPoints (Judgment Call 3 of
// docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md)
// rather than re-deriving them.
async function fetchPoints(
  code: string,
  granularity: TrendGranularity,
  from: string,
  to: string,
): Promise<ChartPoint[]> {
  switch (granularity) {
    case 'day': {
      const stats = await getOperatorDailyStats(code, londonDayKey(from), londonDayKey(to));
      return toChartPoints(stats, (row) => row.day, SPARSE_FLOOR.day);
    }
    case 'halfHour': {
      const stats = await getOperatorHalfHourlyStats(code, from, to);
      return toChartPoints(stats, (row) => row.halfHourStart, SPARSE_FLOOR.halfHour);
    }
    case 'hour': {
      const stats = await getOperatorHourlyStats(code, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.hour);
    }
    case 'sixHour': {
      const stats = await getOperatorSixHourlyStats(code, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.sixHour);
    }
  }
}

/** Operator-scoped sibling of `TrendsResults` -- same fetch/error/empty-state
 * shape, `code` in place of `id`. See this plan's Judgment Call 3 for why
 * this is a new sibling component, not a `scope`-branching rewrite of
 * `TrendsResults` itself. */
export async function OperatorTrendsResults({
  code,
  from,
  to,
  granularity = 'day',
}: {
  code: string;
  from: string;
  to: string;
  granularity?: TrendGranularity;
}) {
  let points: ChartPoint[];
  try {
    points = await fetchPoints(code, granularity, from, to);
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
        <Text c="dimmed">
          Not enough sampled data yet for this operator. If this operator&apos;s lines are TfL-operated, this
          may never populate -- TfL lines don&apos;t currently feed this rollup.
        </Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      <Text size="sm" c="dimmed">
        {HONESTY_COPY[granularity]} Rates shown are summed across every line this operator runs (excluding
        any private custom lines, which never appear in a public rollup).
      </Text>
      <TrendsCharts points={points} granularity={granularity} order={2} showVolume />
    </Stack>
  );
}
