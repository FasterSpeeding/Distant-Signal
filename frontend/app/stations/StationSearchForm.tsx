'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Autocomplete, Button, Group } from '@mantine/core';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

export function StationSearchForm() {
  const router = useRouter();
  const [crs, setCrs] = useState('');
  const { suggestions } = useSuggestions(crs, searchStations);

  function handleSearch() {
    const trimmed = crs.trim().toUpperCase();
    if (!trimmed) return;
    router.push(`/stations/${trimmed}`);
  }

  return (
    <Group align="end">
      <Autocomplete
        label="Station CRS code"
        placeholder="e.g. WOK"
        value={crs}
        onChange={setCrs}
        maxLength={3}
        data={suggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
      />
      <Button onClick={handleSearch} disabled={crs.trim().length === 0}>
        Look up
      </Button>
    </Group>
  );
}
