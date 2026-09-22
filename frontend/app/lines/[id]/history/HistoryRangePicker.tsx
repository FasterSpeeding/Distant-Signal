'use client';

import { useEffect, useId, useState } from 'react';
import { useRouter } from 'next/navigation';
import { DatePickerInput } from '@mantine/dates';
import { Button, Group, SegmentedControl, Stack, Text } from '@mantine/core';
import type { RangePreset } from '@/lib/history';

/** The one selector state this control's `SegmentedControl` actually needs
 * -- a `RangePreset` plus a fourth option with no `lib/history.ts`
 * equivalent, since "the user is looking at the date picker" is UI-only
 * state that never reaches the URL as its own value (a picked custom range
 * lands in the URL as plain `from`/`to`, matching `resolveRange`'s own
 * `preset: null` for that case). */
type Selection = RangePreset | 'custom';

function toCalendarDay(iso: string): string {
  return iso.slice(0, 10);
}

/** `preset`/`from`/`to` come from the page, which resolved them out of the
 * URL (see `lib/history.ts`'s `resolveRange`). The picker is now a display
 * of the range the results below actually cover, not an independent piece
 * of state — the two used to be able to disagree, and the quick-range
 * buttons looked identical whether or not their range was the one showing.
 *
 * Review §2.13 ("one idiom for pick exactly one"): this used to be a row of
 * filled/light preset buttons sitting above a date picker and its own
 * "Show history" submit button -- three controls for one value, two of
 * which applied immediately (the buttons) and one of which needed a
 * separate submit (the picker), with no state at all for "I hand-edited a
 * date" (the buttons all read as unselected, but so does a freshly-loaded
 * default). Collapsed to a single `SegmentedControl` -- 7 days / 30 days /
 * Custom… -- with the picker and its submit button only shown once
 * "Custom…" is picked. That both fixes the undefined "no preset
 * highlighted" state (Custom is always the correct, real answer once a
 * date has been hand-edited) and recovers the picker's vertical space on
 * every render that isn't actually using it. */
export function HistoryRangePicker({
  basePath,
  preset,
  from,
  to,
}: {
  basePath: string;
  preset: RangePreset | null;
  from: string;
  to: string;
}) {
  const router = useRouter();
  const periodLabelId = useId();
  const [value, setValue] = useState<[string | null, string | null]>([
    toCalendarDay(from),
    toCalendarDay(to),
  ]);
  // Mirrors `value`'s own resync rationale below: a `RangePreset` prop can
  // change (a preset click navigates, then the page re-renders this same
  // mounted instance with a fresh `preset`) without the component
  // remounting, so a plain `useState` initialiser would go stale. `preset`
  // is only ever `null` when `resolveRange` fell through to a genuine
  // custom `from`/`to` -- the correct display for that is "Custom…", not
  // "no selection".
  const [selection, setSelection] = useState<Selection>(preset ?? 'custom');

  // `useState`'s initializer only runs once, at mount — but the page
  // re-renders this component with fresh `from`/`to` on every client-side
  // navigation (e.g. clicking a preset button) without remounting it. Left
  // alone, the calendar kept showing the range from whenever the component
  // first mounted, silently disagreeing with both the preset buttons above
  // it (which read `preset` fresh every render) and the results below —
  // exactly the "picker and results can disagree" failure this rewrite
  // exists to fix. Worse, `handleSearch` builds its navigation URL from
  // this same stale `value`, so "Show history" after a preset click could
  // submit the old range. Resyncing here keeps `value` a live mirror of the
  // URL-resolved range rather than a one-time snapshot of it.
  useEffect(() => {
    setValue([toCalendarDay(from), toCalendarDay(to)]);
  }, [from, to]);

  // Only resynced off `preset` itself (not `from`/`to`), and deliberately:
  // picking "Custom…" locally, before a range has been submitted, must not
  // get clobbered back to the old preset just because this effect re-ran
  // for some unrelated reason -- `preset` hasn't actually changed yet in
  // that case, so the dependency array never fires.
  useEffect(() => {
    setSelection(preset ?? 'custom');
  }, [preset]);

  function handleSearch() {
    const [start, end] = value;
    if (!start || !end) return;
    router.push(
      `${basePath}?from=${new Date(start).toISOString()}&to=${new Date(end).toISOString()}`,
    );
  }

  // Presets navigate by name, not by baked-in instants, so a shared link
  // keeps meaning "the last 7 days".
  function handlePreset(next: RangePreset) {
    router.push(`${basePath}?range=${next}`);
  }

  function handleSelectionChange(next: string) {
    if (next === 'custom') {
      // No navigation yet -- this only reveals the picker. `handleSearch`
      // is the thing that actually changes the URL, once both ends are
      // picked.
      setSelection('custom');
      return;
    }
    handlePreset(next as RangePreset);
  }

  const bothEndsPicked = Boolean(value[0] && value[1]);

  return (
    <Stack gap="xs">
      <Stack gap={4}>
        <Text id={periodLabelId} size="xs" fw={600} c="dimmed">
          Period
        </Text>
        <SegmentedControl
          aria-labelledby={periodLabelId}
          color="grape"
          // `SegmentedControl`'s active-label text colour is decided by
          // Mantine's `getContrastColor`, called directly here (not routed
          // through `lib/theme.ts`'s `variantColorResolver`, which only
          // covers Button/Badge/etc.'s "filled" variant) -- and it has the
          // exact scheme-blind bug that resolver's own big comment documents
          // for gray/blue: `parseThemeColor` is given no `colorScheme`, so it
          // always evaluates contrast against `theme.colors.grape[6]`
          // (primaryShade "light" unconditionally), while the background it
          // actually paints on is grape 7 -- `app/globals.css`'s light-scheme
          // substitution for `--mantine-color-grape-filled`, which
          // `getThemeColor` (also colorScheme-blind) resolves to. Grape 6 is
          // light enough that autoContrast picks black; black on the
          // rendered grape 7 is 4.33:1, short of AA's 4.5:1. Pinning
          // `autoContrast={false}` forces the unconditional-white fallback
          // instead, matching the white text `variantColorResolver` already
          // pins for every other grape-filled surface in this app (grape 7
          // white 4.85:1 in light, grape 8 white 5.82:1 in dark -- both
          // pass).
          autoContrast={false}
          value={selection}
          onChange={handleSelectionChange}
          data={[
            { label: '7 days', value: '7d' },
            { label: '30 days', value: '30d' },
            { label: 'Custom…', value: 'custom' },
          ]}
        />
      </Stack>
      {selection === 'custom' && (
        <>
          <Group align="end">
            <DatePickerInput
              type="range"
              label="Pick a date range"
              placeholder="Pick dates range"
              value={value}
              onChange={setValue}
              // The calendar gave no anchor for "where am I" — today rendered
              // exactly like every other day.
              highlightToday
            />
            <Button onClick={handleSearch} disabled={!bothEndsPicked}>
              Show history
            </Button>
          </Group>
          {/* Only while the user has genuinely half-picked a range. It used to
              sit under an empty page as the only thing on it. */}
          {!bothEndsPicked && (
            <Text size="sm" c="dimmed">
              Pick both a start and end date to continue.
            </Text>
          )}
        </>
      )}
    </Stack>
  );
}
