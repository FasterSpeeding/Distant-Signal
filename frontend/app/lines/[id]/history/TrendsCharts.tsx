'use client';

import { LineChart } from '@mantine/charts';
import { Stack, Title } from '@mantine/core';
import { ReferenceArea } from 'recharts';
import type { ChartPoint } from './chartPoint';

/** A contiguous run of one or more days where every rate/delay field is
 * `null` -- i.e. below `SPARSE_DATA_FLOOR_CYCLES` (`TrendsResults.tsx`'s
 * `toChartPoints`, lines 23-35). Checking `delayRate === null` alone is
 * sufficient today because `toChartPoints` guarantees all four fields are
 * nulled together for a sparse day -- an implicit coupling to that
 * invariant, not something `ChartPoint`'s own type enforces (design spec
 * Open question 5). If a future change ever nulls fields independently,
 * this derivation would need to change too. */
export function gapSpans(points: { day: string; delayRate: number | null }[]): { startDay: string; endDay: string }[] {
  const spans: { startDay: string; endDay: string }[] = [];
  let current: { startDay: string; endDay: string } | null = null;
  for (const point of points) {
    if (point.delayRate === null) {
      current = current ? { startDay: current.startDay, endDay: point.day } : { startDay: point.day, endDay: point.day };
    } else {
      if (current) spans.push(current);
      current = null;
    }
  }
  if (current) spans.push(current);
  return spans;
}

/** Widens a `gapSpans` span into the actual `x1`/`x2` values handed to
 * `<ReferenceArea>`. A live render (Task 7's screenshot verification pass,
 * flagged as a risk to check by this comment's own earlier draft -- Task 4
 * Step 3 / design spec Open question 4) confirmed the real failure mode:
 * on `@mantine/charts`' `LineChart` (a Recharts point-scale category axis,
 * not a banded one), an isolated single-day span's `x1 === x2` renders no
 * `<ReferenceArea>` `<path>` at all -- not merely a thin sliver, nothing
 * -- because a point scale has no bandwidth to give a single category. A
 * multi-day span (`x1 !== x2`) is unaffected and renders correctly as-is.
 * There is no real "midpoint" category to anchor a precise half-day band
 * to on a point scale -- the only coordinates Recharts resolves are actual
 * `day` values already in `data` -- so this widens an isolated span to its
 * immediate neighbor(s) instead, the closest a point-scale axis can get to
 * a visible highlight for one point. This is still "one mechanism" in
 * Decision 4's sense (no isolated-day-specific rendering branch,
 * `<ReferenceArea>` used identically either way) -- only the coordinates
 * fed to it differ, and only because the platform itself has no zero-width
 * concept to fall back on. */
function referenceAreaBounds(
  span: { startDay: string; endDay: string },
  points: { day: string }[],
): { x1: string; x2: string } {
  if (span.startDay !== span.endDay) return { x1: span.startDay, x2: span.endDay };
  const idx = points.findIndex((point) => point.day === span.startDay);
  const prev = idx > 0 ? points[idx - 1].day : span.startDay;
  const next = idx >= 0 && idx < points.length - 1 ? points[idx + 1].day : span.startDay;
  return { x1: prev, x2: next };
}

/** Split out of `TrendsResults` (an async Server Component) purely because
 * of `valueFormatter` below: it's a plain function, and Next's RSC
 * serialization refuses to pass a function prop from a Server Component
 * across the boundary into a Client Component (`@mantine/charts`' own
 * `LineChart` carries a `"use client"` directive) -- "Functions cannot be
 * passed directly to Client Components unless you explicitly expose it by
 * marking it with 'use server'." This is a real crash, not a lint nit:
 * confirmed live against a running dev server, both here and on
 * `/lines/[id]/history`'s Trends tab (the exact same `<LineChart
 * valueFormatter={...}>` call, unchanged since before this file existed) --
 * both 500'd with this same digest before this split. The repo's vitest
 * suite never caught it because its `@mantine/charts` mock (see
 * `TrendsResults.test.tsx`) renders everything as one ordinary client tree
 * in jsdom, which never enforces the real RSC server/client serialization
 * boundary a production `next start`/`next dev` request goes through.
 *
 * `points` is the only thing crossing the boundary now, and it's plain
 * data (strings/numbers/null) -- always serializable. Everything else
 * about the two charts (including `valueFormatter` itself) now lives
 * entirely on the client side of that boundary. */
export function TrendsCharts({ points }: { points: ChartPoint[] }) {
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
          dataKey="day"
          withLegend
          series={[
            { name: 'delayRate', label: 'Delay rate', color: 'blue.6' },
            { name: 'cancellationRate', label: 'Cancellation rate', color: 'red.6', strokeDasharray: '6 4' },
            { name: 'skipRate', label: 'Skip rate', color: 'yellow.6', strokeDasharray: '2 3' },
          ]}
          valueFormatter={(value) => `${(value * 100).toFixed(1)}%`}
          connectNulls={false}
          xAxisProps={{ padding: { right: 12 } }}
        >
          {gapSpans(points).map((span) => {
            const { x1, x2 } = referenceAreaBounds(span, points);
            return (
              <ReferenceArea
                key={`${span.startDay}-${span.endDay}`}
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
          dataKey="day"
          series={[{ name: 'avgDelayMinutes', label: 'Avg delay (minutes)', color: 'grape.6' }]}
          valueFormatter={(value) => `${value.toFixed(1)} min`}
          connectNulls={false}
          xAxisProps={{ padding: { right: 12 } }}
        >
          {gapSpans(points).map((span) => {
            const { x1, x2 } = referenceAreaBounds(span, points);
            return (
              <ReferenceArea
                key={`${span.startDay}-${span.endDay}`}
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
