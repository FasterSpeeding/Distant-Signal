'use client';

import dynamic from 'next/dynamic';
import { Box, Stack, Title, type TitleOrder } from '@mantine/core';
import type { ReactNode } from 'react';
import {
  bucketTickLabel,
  bucketTooltipLabel,
  CHART_COLORS,
  CHART_CURVE,
  formatCount,
  formatMinutes,
  formatRate,
} from '@/lib/chartTheme';
import type { TrendGranularity } from '@/lib/history';
import type { ChartPoint } from './chartPoint';
import type { ChartGap } from './TrendsChartParts';

// Recharts and `@mantine/charts` are a large chunk that only these charts
// use, so they load on demand. Client-only (`ssr: false`): Recharts'
// responsive container has no size to draw at on the server anyway.
// `ChartSlot` holds each chart's height while the chunk loads, so nothing
// below it moves when the chart appears.
const TrendsBarChart = dynamic(() => import('./TrendsChartParts').then((m) => m.TrendsBarChart), {
  ssr: false,
});
const TrendsLineChart = dynamic(() => import('./TrendsChartParts').then((m) => m.TrendsLineChart), {
  ssr: false,
});

/** Reserves a chart's `h` before (and while) it renders. */
function ChartSlot({ h, children }: { h: number; children: ReactNode }) {
  return <Box mih={h}>{children}</Box>;
}

/** A contiguous run of one or more buckets where every rate/delay field is
 * `null` -- i.e. below the caller's own sparse-data floor (see
 * `TrendsResults.tsx`'s/`HalfHourlyTrendsResults.tsx`'s own `toChartPoints`-
 * shaped helpers). Checking `delayRate === null` alone is sufficient
 * today because both of those helpers guarantee all four fields are
 * nulled together for a sparse bucket -- an implicit coupling to that
 * invariant, not something `ChartPoint`'s own type enforces. Generalized
 * from day-specific `{ day, startDay, endDay }` naming to
 * `{ bucketKey, startKey, endKey }` -- Decision 9 of
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md;
 * the underlying algorithm is unchanged from the daily-only version. */
export function gapSpans(
  points: { bucketKey: string; delayRate: number | null }[],
): { startKey: string; endKey: string }[] {
  const spans: { startKey: string; endKey: string }[] = [];
  let current: { startKey: string; endKey: string } | null = null;
  for (const point of points) {
    if (point.delayRate === null) {
      current = current
        ? { startKey: current.startKey, endKey: point.bucketKey }
        : { startKey: point.bucketKey, endKey: point.bucketKey };
    } else {
      if (current) spans.push(current);
      current = null;
    }
  }
  if (current) spans.push(current);
  return spans;
}

/** Widens a `gapSpans` span into the actual `x1`/`x2` values handed to
 * `<ReferenceArea>` -- unchanged in substance from the daily-only
 * version (see its prior doc comment, preserved in git history), only
 * the field names are generalized. Still needed for both granularities:
 * `@mantine/charts`' `LineChart` is a Recharts point-scale category axis
 * regardless of whether the category values are day strings or hour-start
 * instants, so an isolated single-bucket gap still needs widening to its
 * neighbors to render at all. */
function referenceAreaBounds(
  span: { startKey: string; endKey: string },
  points: { bucketKey: string }[],
): { x1: string; x2: string } {
  if (span.startKey !== span.endKey) return { x1: span.startKey, x2: span.endKey };
  const idx = points.findIndex((point) => point.bucketKey === span.startKey);
  // Past either end (or with no match), the neighbour is missing and the
  // span keeps its own key on that side.
  const prev = (idx > 0 ? points[idx - 1] : undefined)?.bucketKey ?? span.startKey;
  const next = (idx >= 0 ? points[idx + 1] : undefined)?.bucketKey ?? span.startKey;
  return { x1: prev, x2: next };
}

/** Split out of `TrendsResults`/`HalfHourlyTrendsResults` (both `async`
 * Server Components) purely because of `valueFormatter` below: a plain
 * function, and Next's RSC serialization refuses to pass a function prop
 * from a Server Component across the boundary into a Client Component.
 * See git history for the full incident this originally fixed --
 * unchanged by this generalization.
 *
 * `granularity` is new: a plain, serializable `TrendGranularity` string
 * (`'halfHour' | 'hour' | 'sixHour' | 'day'`, `frontend/lib/history.ts`)
 * (never a function, so it crosses the Server/Client boundary safely from
 * either caller) that controls ONLY the x-axis tick label formatting.
 * `points[].bucketKey` stays the raw, always-unique category identity for
 * EVERY granularity (a "YYYY-MM-DD" day string, or an RFC3339
 * bucket-start instant) -- any non-`'day'` value additionally
 * renders each tick through `formatTime` (e.g. "14:30") for a legible
 * axis, without changing what Recharts uses as the category key. This
 * split matters because a rolling 24-hour window's wall-clock
 * time-of-day label can legitimately repeat once (yesterday's and
 * today's same clock time) whenever the window straddles a day boundary
 * -- using a formatted label as the category KEY itself would silently
 * collide two distinct buckets. (Originally `'day' | 'hour'`, for a
 * 1-hour bucket -- renamed to `'halfHour'` alongside the rest of this
 * feature when the bucket size was halved; the collision risk and its
 * fix are unchanged, just at double the bucket count.)
 *
 * `showVolume` is a third, independent prop: when `true`, an additional
 * "Trains counted" `<BarChart>` panel renders first, above the rate
 * chart, reading `points[].total` directly -- see this file's own render
 * logic and
 * docs/superpowers/plans/2026-09-05-trend-sample-volume-chart-plan.md. */
export function TrendsCharts({
  points,
  granularity,
  order,
  showVolume = false,
}: {
  points: ChartPoint[];
  granularity: TrendGranularity;
  /** Heading level for the two chart titles. Required, deliberately
   * undefaulted: this component is mounted at two different depths
   * (h2 on /lines/[id]/history's Trends tab, h3 under "Recent trends" on
   * /lines/[id]), so any default would render a heading-order skip on one
   * of them -- which is exactly the axe `heading-order` defect this
   * replaced (both titles were a hardcoded `order={4}`). `size="h6"` stays
   * pinned at both call sites, so changing the level changes the tag only,
   * never the rendered font size. */
  order: TitleOrder;
  /** Renders the "Trains counted" bar-chart panel above the rate chart
   * when `true`. Defaults to `false` -- only the two LDBWS-sample-backed
   * callers (`TrendsResults`, `HalfHourlyTrendsResults`) pass `true`. The
   * full-coverage callers (`CoverageTrendsResults`,
   * `HalfHourlyCoverageTrendsResults`) deliberately do not, since that
   * rollup's `total` isn't deduplicated yet -- see
   * docs/superpowers/plans/2026-09-05-trend-sample-volume-chart-plan.md's
   * "Scope decision: full-coverage". */
  showVolume?: boolean;
}) {
  // Review §3.3: the day axis used to print the raw "2026-09-15" bucket
  // key. Ticks and tooltip headings now go through `lib/chartTheme.ts`
  // ("15 Sep", "14:30"; "15 Sep 2026", "15 Sep, 14:30").
  const xAxisProps = {
    padding: { right: 12 },
    tickFormatter: (value: string) => bucketTickLabel(granularity, value),
  };
  const tooltipProps = {
    labelFormatter: (label: unknown) => (typeof label === 'string' ? bucketTooltipLabel(granularity, label) : ''),
  };
  const gaps: ChartGap[] = gapSpans(points).map((span) => ({
    key: `${span.startKey}-${span.endKey}`,
    ...referenceAreaBounds(span, points),
  }));

  return (
    <>
      {showVolume && (
        <Stack gap={4}>
          <Title order={order} size="h6">
            Trains counted
          </Title>
          {/* Single flat series, never a stack of delayed/cancelled/skipped
              against total -- those three are independently-computed,
              overlapping filters over total (crates/common/src/lib.rs's
              compute_sample_stats), not a partition of it; stacking them
              would visually exceed total and mislead. No gapSpans/
              ReferenceArea here (unlike the two panels below): `total` is
              never null for a bucket with a real row -- see this plan's
              "Judgment call" section for why a low bar for a sparse bucket
              is the informative signal, not something to hide. */}
          <ChartSlot h={180}>
            <TrendsBarChart
              h={180}
              data={points}
              dataKey="bucketKey"
              series={[{ name: 'total', label: 'Trains counted', color: CHART_COLORS.count }]}
              valueFormatter={formatCount}
              xAxisProps={xAxisProps}
              tooltipProps={tooltipProps}
            />
          </ChartSlot>
        </Stack>
      )}
      <Stack gap={4}>
        <Title order={order} size="h6">
          Delay, cancellation and skip rates
        </Title>
        {/* The three rate metrics are 0-1 proportions and share one chart/axis.
            Average delay minutes is a different unit and is deliberately never
            combined onto this chart or its axis -- see the second LineChart
            below, and the plan's Global Constraints. The dash patterns keep
            the three apart without relying on colour. */}
        <ChartSlot h={310}>
          <TrendsLineChart
            h={310}
            data={points}
            dataKey="bucketKey"
            withLegend
            curveType={CHART_CURVE}
            series={[
              { name: 'delayRate', label: 'Delay rate', color: CHART_COLORS.delay },
              {
                name: 'cancellationRate',
                label: 'Cancellation rate',
                color: CHART_COLORS.cancellation,
                strokeDasharray: '6 4',
              },
              { name: 'skipRate', label: 'Skip rate', color: CHART_COLORS.skip, strokeDasharray: '2 3' },
            ]}
            valueFormatter={formatRate}
            connectNulls={false}
            xAxisProps={xAxisProps}
            tooltipProps={tooltipProps}
            gaps={gaps}
          />
        </ChartSlot>
      </Stack>
      <Stack gap={4}>
        <Title order={order} size="h6">
          Average delay (minutes)
        </Title>
        <ChartSlot h={220}>
          <TrendsLineChart
            h={220}
            data={points}
            dataKey="bucketKey"
            curveType={CHART_CURVE}
            series={[{ name: 'avgDelayMinutes', label: 'Average delay', color: CHART_COLORS.delay }]}
            valueFormatter={formatMinutes}
            connectNulls={false}
            xAxisProps={xAxisProps}
            tooltipProps={tooltipProps}
            gaps={gaps}
          />
        </ChartSlot>
      </Stack>
    </>
  );
}
