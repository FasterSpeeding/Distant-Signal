'use client';

import { useId } from 'react';
import { useRouter } from 'next/navigation';
import { SegmentedControl, Stack, Text } from '@mantine/core';
import type { RangePreset, TrendGranularity } from '@/lib/history';

const LABELS: Record<TrendGranularity, string> = {
  halfHour: '30 min',
  hour: 'Hourly',
  sixHour: '6-hourly',
  day: 'Daily',
};

// Finest to coarsest -- matches frontend/lib/history.ts's own
// GRANULARITY_ORDER. Duplicated here (not imported) because it's a plain
// display-order constant with no logic attached; `available` (computed by
// page.tsx via history.ts's own availableGranularities) is the actual
// source of truth for which tiers show up at all.
const DISPLAY_ORDER: TrendGranularity[] = ['halfHour', 'hour', 'sixHour', 'day'];

/** Renders a `SegmentedControl` scoped to the Trends `TabsPanel` (Decision
 * 6 of docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
 * -- deliberately NOT added to `HistoryRangePicker.tsx`, since granularity
 * has no meaning on the Timeline tab). An unavailable tier is OMITTED from
 * `data` entirely rather than rendered disabled -- following the one
 * existing precedent for a conditional `SegmentedControl` option in this
 * codebase (`components/IssueList.tsx`'s "Ended" bucket, "only offered
 * when something is actually in it"). `available` always includes `'day'`
 * (see `availableGranularities`'s own doc comment), so `data` is never
 * empty. State lives in the URL, matching every other piece of range state
 * on this page (`HistoryRangePicker.tsx`'s own `handlePreset`/`handleSearch`
 * convention) -- switching tiers navigates with the SAME range params
 * (`preset`, or `from`/`to`) plus a `?granularity=` param, never losing the
 * currently-viewed date range. */
export function GranularityControl({
  basePath,
  preset,
  from,
  to,
  granularity,
  available,
}: {
  basePath: string;
  preset: RangePreset | null;
  from: string;
  to: string;
  granularity: TrendGranularity;
  available: TrendGranularity[];
}) {
  const router = useRouter();
  const granularityLabelId = useId();
  const unavailable = DISPLAY_ORDER.filter((g) => !available.includes(g));

  function handleChange(value: string) {
    const rangeParams = preset ? `range=${preset}` : `from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`;
    router.push(`${basePath}?${rangeParams}&granularity=${value}`);
  }

  return (
    <Stack gap={4}>
      {/* Visible label + `aria-labelledby`, and `color="grape"`, copied
          from the "Period" control this one sits directly beneath
          (`HistoryRangePicker.tsx`). Before this, a screen-reader user
          reached an unnamed radiogroup and heard three bare options, and
          a sighted user's only hint was the dimmed helper text BELOW the
          control -- which explains an ABSENT option before naming the
          present ones (2026-09-22 UX review, I9/P4). The two controls also
          read as two unrelated widgets while one was grape-filled and the
          other white-on-grey; they are two settings of one chart, so they
          now share one idiom (09-17 §2.13 asked for this; only the Period
          half had been done). */}
      <Text id={granularityLabelId} size="xs" fw={600} c="dimmed">
        Granularity
      </Text>
      <SegmentedControl
        aria-labelledby={granularityLabelId}
        color="grape"
        // Same reasoning as `HistoryRangePicker`'s own
        // `autoContrast={false}` -- see its comment: Mantine's
        // `getContrastColor` is scheme-blind here and picks black on the
        // grape 7 this app substitutes, which is 4.33:1, under AA.
        autoContrast={false}
        value={granularity}
        onChange={handleChange}
        data={DISPLAY_ORDER.filter((g) => available.includes(g)).map((g) => ({ label: LABELS[g], value: g }))}
      />
      {unavailable.length > 0 && (
        <Text size="xs" c="dimmed">
          {/* Built as a single template literal, not line-wrapped JSX text, deliberately: line-wrapped
              text immediately after a `{expr}` collapses inconsistently between this repo's two JSX
              transforms -- Next's SWC (the real dev/prod bundler) drops the space right after the
              ternary below ("isnot shown"/"arenot shown"), while Vitest's esbuild-based transform kept
              it, so the bug passed every unit test and only showed up live (confirmed via a real dev
              server, textContent, not just the accessibility-tree snapshot). One template literal has
              no line-wrap boundary for either transform to collapse differently. */}
          {`${unavailable.map((g) => LABELS[g]).join(', ')} ${unavailable.length === 1 ? 'is' : 'are'} not shown for this range -- it's wider than what's retained at that granularity, or would render too many points to read clearly.`}
        </Text>
      )}
    </Stack>
  );
}
