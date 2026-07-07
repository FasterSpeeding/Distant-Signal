'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { DatePickerInput } from '@mantine/dates';
import { Button, Group } from '@mantine/core';

export function HistoryRangePicker({ lineId }: { lineId: string }) {
  const router = useRouter();
  const [value, setValue] = useState<[string | null, string | null]>([null, null]);

  function handleSearch() {
    const [from, to] = value;
    if (!from || !to) return;
    const fromIso = new Date(from).toISOString();
    const toIso = new Date(to).toISOString();
    router.push(`/lines/${lineId}/history?from=${fromIso}&to=${toIso}`);
  }

  return (
    <Group align="end">
      <DatePickerInput
        type="range"
        label="Pick a date range"
        placeholder="Pick dates range"
        value={value}
        onChange={setValue}
      />
      <Button onClick={handleSearch} disabled={!value[0] || !value[1]}>
        Show history
      </Button>
    </Group>
  );
}
