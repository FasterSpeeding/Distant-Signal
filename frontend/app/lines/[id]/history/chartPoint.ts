/** Shared between `TrendsResults.tsx` (the Server Component that fetches
 * the data and derives these) and `TrendsCharts.tsx` (the Client Component
 * that actually renders them) -- pulled into its own module, rather than
 * one file importing the type from the other, to avoid a Server/Client
 * pair importing each other at all. */
export interface ChartPoint {
  day: string;
  delayRate: number | null;
  cancellationRate: number | null;
  skipRate: number | null;
  avgDelayMinutes: number | null;
  sampleCycles: number;
}
