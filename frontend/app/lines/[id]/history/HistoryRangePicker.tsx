'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { DatePickerInput } from '@mantine/dates';
import { Button, Group, Stack, Text } from '@mantine/core';

const DAY_MS = 24 * 60 * 60 * 1000;

export function HistoryRangePicker({ lineId }: { lineId: string }) {
  const router = useRouter();
  const [value, setValue] = useState<[string | null, string | null]>([null, null]);

  function goToRange(from: string, to: string) {
    router.push(`/lines/${lineId}/history?from=${from}&to=${to}`);
  }

  function handleSearch() {
    const [from, to] = value;
    if (!from || !to) return;
    goToRange(new Date(from).toISOString(), new Date(to).toISOString());
  }

  // "Did my line misbehave recently" is the page's main use case, so these
  // presets skip the calendar entirely and jump straight to results rather
  // than just populating the picker.
  function handlePreset(days: number) {
    const now = new Date();
    const from = new Date(now.getTime() - days * DAY_MS);
    goToRange(from.toISOString(), now.toISOString());
  }

  const bothEndsPicked = Boolean(value[0] && value[1]);

  return (
    <Stack gap="xs">
      <Group gap="sm">
        <Button variant="light" onClick={() => handlePreset(7)}>
          Last 7 days
        </Button>
        <Button variant="light" onClick={() => handlePreset(30)}>
          Last 30 days
        </Button>
      </Group>
      <Group align="end">
        <DatePickerInput
          type="range"
          label="Pick a date range"
          placeholder="Pick dates range"
          value={value}
          onChange={setValue}
        />
        <Button onClick={handleSearch} disabled={!bothEndsPicked}>
          Show history
        </Button>
      </Group>
      {!bothEndsPicked && <Text size="sm" c="dimmed">Pick both a start and end date to continue.</Text>}
    </Stack>
  );
}
