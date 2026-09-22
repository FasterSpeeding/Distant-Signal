import { Paper, Stack, Text } from '@mantine/core';
import { getLineDailyStats, getLineHalfHourlyStats, getLineHourlyStats, getLineSixHourlyStats } from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholders, not validated numbers -- see
// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
// Decision 5. `day`/`halfHour` are unchanged from their prior standalone
// constants (`SPARSE_DATA_FLOOR_CYCLES`/`SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`
// in this file's and HalfHourlyTrendsResults.tsx's git history);
// `hour`/`sixHour` are newly derived by the same "~third of the bucket's
// max possible poll-cycle coverage" rule.
export const SPARSE_FLOOR: Record<TrendGranularity, number> = {
  halfHour: 10,
  hour: 20,
  sixHour: 120,
  day: 20,
};

// One honesty-copy sentence per granularity (Ruling A,
// .superpowers/sdd/2026-08-31-line-history-graphics/progress.md, extended
// to the two new tiers by the same template) -- the underlying commitment
// (never draw a misleading flat line over data too sparse to trust) must
// not be softened or dropped, same as this file's pre-existing `day` copy.
//
// Reworded per the 2026-09-22 UX review ([OH] §3.3/I10): the original
// five-sentence-in-one paragraph read as a developer's own note to
// themselves -- "poll cycles", "coverage", `--` for a dash, all inherited
// verbatim from this constant's OWN prior doc comment above, which is
// exactly the leak review §3.3 called out. Split in two: this one short,
// plain-language sentence stays inline next to the charts; the fuller
// mechanical explanation (still saying everything the original one did)
// moved to `HONESTY_COPY_DETAILS` below, behind a collapsed "How these
// rates are calculated" disclosure (`TrendsResults`'s own render).
export const HONESTY_COPY: Record<TrendGranularity, string> = {
  day: 'Each train is counted once per day, by the status it had when first seen. Days with too little data are left blank rather than shown as a misleading flat line.',
  halfHour:
    'Each train is counted once per half hour, by the status it had when first seen. Half-hour periods with too little data are left blank rather than shown as a misleading flat line.',
  hour: 'Each train is counted once per hour, by the status it had when first seen. Hours with too little data are left blank rather than shown as a misleading flat line.',
  sixHour:
    'Each train is counted once per six-hour period, by the status it had when first seen. Six-hour periods with too little data are left blank rather than shown as a misleading flat line.',
};

/** The rest of what `HONESTY_COPY` used to say in one paragraph -- same
 * facts, same "must not be softened" commitment, just moved behind a
 * collapsed disclosure rather than printed in full every time (review
 * §3.3's recommendation). Plain `<details>`/`<summary>`, not Mantine's
 * `Spoiler`: this codebase already chose that for exactly this "collapsed
 * extra detail" shape (`app/chat/callback/page.tsx`'s "Show error
 * details") over `Spoiler`, whose own measured-height-gated control and
 * "hidden but still mounted" content are the wrong fit here too (see
 * `components/StationAccessibilitySection.tsx`'s `Disclosure` doc comment
 * for the fuller reasoning against `Spoiler` specifically). */
export const HONESTY_COPY_DETAILS: Record<TrendGranularity, string> = {
  day: "A train that starts on time and only becomes delayed later, while still in view, still counts as on time here — it's the status we saw first, not a running tally. The trains-counted chart above always shows the real number of trains seen that day, even a day too sparse to trust for a rate — a low bar there is exactly why that day may show as a gap in the rate chart below it. It counts each train once, in the day it was first seen, not how many were running at the same time.",
  halfHour:
    "A train that starts on time and only becomes delayed later, while still in view, still counts as on time here — it's the status we saw first, not a running tally. The trains-counted chart above always shows the real number of trains seen that half hour, even a half hour too sparse to trust for a rate — a low bar there is exactly why that half hour may show as a gap in the rate chart below it. It counts each train once, in the half hour it was first seen, not how many were running at the same time.",
  hour: "A train that starts on time and only becomes delayed later, while still in view, still counts as on time here — it's the status we saw first, not a running tally. The trains-counted chart above always shows the real number of trains seen that hour, even an hour too sparse to trust for a rate — a low bar there is exactly why that hour may show as a gap in the rate chart below it. It counts each train once, in the hour it was first seen, not how many were running at the same time.",
  sixHour:
    "A train that starts on time and only becomes delayed later, while still in view, still counts as on time here — it's the status we saw first, not a running tally. The trains-counted chart above always shows the real number of trains seen in that six-hour period, even a period too sparse to trust for a rate — a low bar there is exactly why that period may show as a gap in the rate chart below it. It counts each train once, in the period it was first seen, not how many were running at the same time.",
};

interface StatsRow {
  sampleCycles: number;
  total: number;
  delayRate: number;
  cancellationRate: number;
  skipRate: number;
  avgDelayMinutes: number;
}

// Generalized from the original day-only toChartPoints: same
// null-all-four-fields-together gap logic (Decision 3 of
// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md), now
// parameterized over which row field supplies `bucketKey` and which floor
// applies, so one function serves all four granularities' differently-shaped
// row types (LineDailyStats.day, LineHalfHourlyStats.halfHourStart,
// LineHourlyStats/LineSixHourlyStats.bucketStart).
export function toChartPoints<T extends StatsRow>(
  stats: T[],
  bucketKeyOf: (row: T) => string,
  floor: number,
): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < floor;
    return {
      bucketKey: bucketKeyOf(row),
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.sampleCycles,
    };
  });
}

// Dispatches to the right fetch + floor + bucket-key field for the
// selected tier (Decision 4's own sketch). `day` still converts its
// RFC3339 `from`/`to` to London calendar-day keys first, exactly as
// before -- the only granularity whose route takes NaiveDate path segments.
async function fetchPoints(id: string, granularity: TrendGranularity, from: string, to: string): Promise<ChartPoint[]> {
  switch (granularity) {
    case 'day': {
      const stats = await getLineDailyStats(id, londonDayKey(from), londonDayKey(to));
      return toChartPoints(stats, (row) => row.day, SPARSE_FLOOR.day);
    }
    case 'halfHour': {
      const stats = await getLineHalfHourlyStats(id, from, to);
      return toChartPoints(stats, (row) => row.halfHourStart, SPARSE_FLOOR.halfHour);
    }
    case 'hour': {
      const stats = await getLineHourlyStats(id, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.hour);
    }
    case 'sixHour': {
      const stats = await getLineSixHourlyStats(id, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.sixHour);
    }
  }
}

export async function TrendsResults({
  id,
  from,
  to,
  granularity = 'day',
}: {
  id: string;
  from: string;
  to: string;
  /** Defaults to `'day'` -- the existing, always-safe behavior, unchanged
   * for any call site that doesn't pass this yet (Decision 6). */
  granularity?: TrendGranularity;
}) {
  // Reversing Decision 4 of
  // docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
  // also means this component now serves the same backend table the
  // line-info page's HalfHourlyTrendsResults reads for three of its four
  // tiers -- adopting that component's own try/catch degrade-gracefully
  // posture here too (previously this file had none, and an unhandled
  // rejection would have propagated to app/error.tsx, blanking the whole
  // page over a secondary chart) makes error handling consistent across
  // all four tiers rather than arbitrarily different for `day` alone.
  let points: ChartPoint[];
  try {
    points = await fetchPoints(id, granularity, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (points.length === 0) {
    // Live-investigated (docs/superpowers/plans/2026-09-02-line-history-chart-fixes.md
    // Task 6 Step 1): the reported "dead whitespace below the footer" is a
    // transient Suspense loading-flash artifact, not a persistent layout
    // bug. Wrapped in a bounded `Paper` regardless -- it reads as a
    // deliberately-finished component rather than a chart that failed to
    // render.
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet for this line.</Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      <Text size="sm" c="dimmed">
        {HONESTY_COPY[granularity]}
      </Text>
      {/* Both charts (including their `valueFormatter`) live in TrendsCharts,
          a Client Component -- see its own doc comment for why: a plain
          function prop can't cross the Server-to-Client boundary straight
          out of this `async` Server Component. */}
      {/* order={2}: this sits directly under /lines/[id]/history's only
          h1 ("History: {name}"), with nothing between -- h2 keeps the
          chart titles one level below that h1, with no skip. */}
      <TrendsCharts points={points} granularity={granularity} order={2} showVolume />
      {/* Review §3.3: the data now leads, with the fuller mechanical
          explanation collapsed below it rather than printed in full above
          the charts every time. */}
      <details>
        <summary>How these rates are calculated</summary>
        <Text size="sm" c="dimmed" mt="xs">
          {HONESTY_COPY_DETAILS[granularity]}
        </Text>
      </details>
    </Stack>
  );
}
