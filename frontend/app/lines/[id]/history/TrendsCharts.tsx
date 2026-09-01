'use client';

import { LineChart } from '@mantine/charts';
import { Stack, Title } from '@mantine/core';
import type { ChartPoint } from './chartPoint';

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
    </>
  );
}
