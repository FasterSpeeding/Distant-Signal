'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { TextInput, Button, Group } from '@mantine/core';

export function StationSearchForm() {
  const router = useRouter();
  const [crs, setCrs] = useState('');

  function handleSearch() {
    const trimmed = crs.trim().toUpperCase();
    if (!trimmed) return;
    router.push(`/stations/${trimmed}`);
  }

  return (
    <Group align="end">
      <TextInput
        label="Station CRS code"
        placeholder="e.g. WOK"
        value={crs}
        onChange={(event) => setCrs(event.currentTarget.value)}
        maxLength={3}
      />
      <Button onClick={handleSearch} disabled={crs.trim().length === 0}>
        Look up
      </Button>
    </Group>
  );
}
