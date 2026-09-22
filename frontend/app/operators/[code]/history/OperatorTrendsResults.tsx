import { Paper, Stack, Text } from '@mantine/core';
import {
  getOperatorDailyStats,
  getOperatorHalfHourlyStats,
  getOperatorHourlyStats,
  getOperatorSixHourlyStats,
} from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { HONESTY_COPY, HONESTY_COPY_DETAILS, SPARSE_FLOOR, toChartPoints } from '@/app/lines/[id]/history/TrendsResults';
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
          may never populate — TfL lines aren&apos;t counted here yet.
        </Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      {/* One template literal, not `{HONESTY_COPY[granularity]} Rates shown…`
          split across the expression boundary: `GranularityControl.tsx`'s own
          doc comment records a real, already-diagnosed bug where Next's SWC
          (the real dev/prod bundler) silently drops the space right after a
          `{expr}` in some line-wrap shapes, while Vitest's esbuild-based
          transform does not -- passing every unit test while shipping
          "running.Rates" live (review [OH] §3.3, flagged there as
          "needs verifying against the deployed build"). A single template
          literal has no such boundary for either transform to disagree
          about. */}
      <Text size="sm" c="dimmed">
        {`${HONESTY_COPY[granularity]} Rates shown are summed across every line this operator runs. Private custom lines are never included, since they aren't public.`}
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
