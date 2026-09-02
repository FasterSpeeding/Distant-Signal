'use client';

import { LineChart } from '@mantine/charts';
import { Stack, Title } from '@mantine/core';
import { ReferenceArea } from 'recharts';
import { formatTime } from '@/lib/dateFormat';
import type { ChartPoint } from './chartPoint';

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
export function gapSpans(points: { bucketKey: string; delayRate: number | null }[]): { startKey: string; endKey: string }[] {
  const spans: { startKey: string; endKey: string }[] = [];
  let current: { startKey: string; endKey: string } | null = null;
  for (const point of points) {
    if (point.delayRate === null) {
      current = current ? { startKey: current.startKey, endKey: point.bucketKey } : { startKey: point.bucketKey, endKey: point.bucketKey };
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
  const prev = idx > 0 ? points[idx - 1].bucketKey : span.startKey;
  const next = idx >= 0 && idx < points.length - 1 ? points[idx + 1].bucketKey : span.startKey;
  return { x1: prev, x2: next };
}

/** Split out of `TrendsResults`/`HalfHourlyTrendsResults` (both `async`
 * Server Components) purely because of `valueFormatter` below: a plain
 * function, and Next's RSC serialization refuses to pass a function prop
 * from a Server Component across the boundary into a Client Component.
 * See git history for the full incident this originally fixed --
 * unchanged by this generalization.
 *
 * `granularity` is new: a plain, serializable `'day' | 'halfHour'` string
 * (never a function, so it crosses the Server/Client boundary safely from
 * either caller) that controls ONLY the x-axis tick label formatting.
 * `points[].bucketKey` stays the raw, always-unique category identity for
 * BOTH granularities (a "YYYY-MM-DD" day string, or an RFC3339
 * half-hour-start instant) -- `granularity === 'halfHour'` additionally
 * renders each tick through `formatTime` (e.g. "14:30") for a legible
 * axis, without changing what Recharts uses as the category key. This
 * split matters because a rolling 24-hour window's wall-clock
 * time-of-day label can legitimately repeat once (yesterday's and
 * today's same clock time) whenever the window straddles a day boundary
 * -- using a formatted label as the category KEY itself would silently
 * collide two distinct buckets. (Originally `'day' | 'hour'`, for a
 * 1-hour bucket -- renamed to `'halfHour'` alongside the rest of this
 * feature when the bucket size was halved; the collision risk and its
 * fix are unchanged, just at double the bucket count.) */
export function TrendsCharts({ points, granularity }: { points: ChartPoint[]; granularity: 'day' | 'halfHour' }) {
  const xAxisProps = {
    padding: { right: 12 },
    ...(granularity === 'halfHour' ? { tickFormatter: (value: string) => formatTime(value) } : {}),
  };

  return (
    <>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Delay / cancellation / skip rate
        </Title>
        {/* The three rate metrics are 0-1 proportions and share one chart/axis.
            Average delay minutes is a different unit and is deliberately never
            combined onto this chart or its axis -- see the second LineChart
            below, and the plan's Global Constraints. */}
        <LineChart
          h={310}
          data={points}
          dataKey="bucketKey"
          withLegend
          series={[
            { name: 'delayRate', label: 'Delay rate', color: 'blue.6' },
            { name: 'cancellationRate', label: 'Cancellation rate', color: 'red.6', strokeDasharray: '6 4' },
            { name: 'skipRate', label: 'Skip rate', color: 'yellow.6', strokeDasharray: '2 3' },
          ]}
          valueFormatter={(value) => `${(value * 100).toFixed(1)}%`}
          connectNulls={false}
          xAxisProps={xAxisProps}
        >
          {gapSpans(points).map((span) => {
            const { x1, x2 } = referenceAreaBounds(span, points);
            return (
              <ReferenceArea
                key={`${span.startKey}-${span.endKey}`}
                x1={x1}
                x2={x2}
                fill="var(--mantine-color-gray-5)"
                fillOpacity={0.15}
                stroke="none"
                ifOverflow="visible"
              />
            );
          })}
        </LineChart>
      </Stack>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Average delay (minutes)
        </Title>
        <LineChart
          h={220}
          data={points}
          dataKey="bucketKey"
          series={[{ name: 'avgDelayMinutes', label: 'Avg delay (minutes)', color: 'grape.6' }]}
          valueFormatter={(value) => `${value.toFixed(1)} min`}
          connectNulls={false}
          xAxisProps={xAxisProps}
        >
          {gapSpans(points).map((span) => {
            const { x1, x2 } = referenceAreaBounds(span, points);
            return (
              <ReferenceArea
                key={`${span.startKey}-${span.endKey}`}
                x1={x1}
                x2={x2}
                fill="var(--mantine-color-gray-5)"
                fillOpacity={0.15}
                stroke="none"
                ifOverflow="visible"
              />
            );
          })}
        </LineChart>
      </Stack>
    </>
  );
}
