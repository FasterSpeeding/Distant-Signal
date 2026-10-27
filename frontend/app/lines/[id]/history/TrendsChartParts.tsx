'use client';

import { BarChart, LineChart, type BarChartProps, type LineChartProps } from '@mantine/charts';
import { ReferenceArea } from 'recharts';
import { CHART_GAP_FILL, CHART_GAP_OPACITY } from '@/lib/chartTheme';

/** A shaded span over buckets too sparse to plot (`gapSpans`, widened by
 * `referenceAreaBounds` in `TrendsCharts.tsx`). */
export interface ChartGap {
  key: string;
  x1: string;
  x2: string;
}

// The Recharts-backed half of `TrendsCharts`. Recharts and `@mantine/charts`
// are a large chunk, so `TrendsCharts` loads this module on demand
// (`next/dynamic`) and holds each chart's height until it arrives.

export function TrendsBarChart(props: BarChartProps) {
  return <BarChart {...props} />;
}

export function TrendsLineChart({ gaps, ...props }: Omit<LineChartProps, 'children'> & { gaps: ChartGap[] }) {
  return (
    <LineChart {...props}>
      {gaps.map((gap) => (
        <ReferenceArea
          key={gap.key}
          x1={gap.x1}
          x2={gap.x2}
          fill={CHART_GAP_FILL}
          fillOpacity={CHART_GAP_OPACITY}
          stroke="none"
          ifOverflow="visible"
        />
      ))}
    </LineChart>
  );
}
