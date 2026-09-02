/** Shared between `TrendsResults.tsx`/`HalfHourlyTrendsResults.tsx` (the
 * Server Components that fetch the data and derive these) and
 * `TrendsCharts.tsx` (the Client Component that actually renders them) --
 * pulled into its own module, rather than one file importing the type
 * from the other, to avoid a Server/Client pair importing each other at
 * all.
 *
 * `bucketKey` is deliberately generic, not `day` -- this type now backs
 * both the daily rollup ("YYYY-MM-DD" London calendar-day strings) and
 * the half-hourly rollup (RFC3339 UTC half-hour-start instants, kept as
 * the raw instant string rather than a pre-formatted display label -- see
 * `TrendsCharts.tsx`'s `granularity` prop for why: two different
 * half-hourly buckets can share the same wall-clock time-of-day label
 * across a day boundary, so the category-axis IDENTITY must stay the
 * always-unique raw instant, with display formatting applied separately,
 * only for the tick labels). See
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
 * Decision 9 (written when this was still an hourly rollup -- the
 * reasoning is unchanged at 30-minute buckets). */
export interface ChartPoint {
  bucketKey: string;
  delayRate: number | null;
  cancellationRate: number | null;
  skipRate: number | null;
  avgDelayMinutes: number | null;
  sampleCycles: number;
}
