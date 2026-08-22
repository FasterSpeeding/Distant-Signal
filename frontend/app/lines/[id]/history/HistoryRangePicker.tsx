'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { DatePickerInput } from '@mantine/dates';
import { Button, Group, Stack, Text } from '@mantine/core';
import type { RangePreset } from '@/lib/history';

function toCalendarDay(iso: string): string {
  return iso.slice(0, 10);
}

/** `preset`/`from`/`to` come from the page, which resolved them out of the
 * URL (see `lib/history.ts`'s `resolveRange`). The picker is now a display
 * of the range the results below actually cover, not an independent piece
 * of state — the two used to be able to disagree, and the quick-range
 * buttons looked identical whether or not their range was the one showing. */
export function HistoryRangePicker({
  lineId,
  preset,
  from,
  to,
}: {
  lineId: string;
  preset: RangePreset | null;
  from: string;
  to: string;
}) {
  const router = useRouter();
  const [value, setValue] = useState<[string | null, string | null]>([
    toCalendarDay(from),
    toCalendarDay(to),
  ]);

  function handleSearch() {
    const [start, end] = value;
    if (!start || !end) return;
    router.push(
      `/lines/${lineId}/history?from=${new Date(start).toISOString()}&to=${new Date(end).toISOString()}`,
    );
  }

  // Presets navigate by name, not by baked-in instants, so a shared link
  // keeps meaning "the last 7 days".
  function handlePreset(next: RangePreset) {
    router.push(`/lines/${lineId}/history?range=${next}`);
  }

  const bothEndsPicked = Boolean(value[0] && value[1]);

  function presetProps(candidate: RangePreset) {
    const selected = preset === candidate;
    return {
      // Filled vs light rather than two shades of the same tint: the
      // difference between "selected" and "not" was barely perceptible.
      variant: selected ? ('filled' as const) : ('light' as const),
      'aria-pressed': selected,
      onClick: () => handlePreset(candidate),
    };
  }

  return (
    <Stack gap="xs">
      <Group gap="sm">
        <Button {...presetProps('7d')}>Last 7 days</Button>
        <Button {...presetProps('30d')}>Last 30 days</Button>
      </Group>
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
    </Stack>
  );
}
