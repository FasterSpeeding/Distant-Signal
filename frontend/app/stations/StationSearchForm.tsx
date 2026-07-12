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
        // `data`'s `label` — not `value` — is what Mantine's Autocomplete
        // writes into the field on selection (confirmed by reading its
        // source: `handleValueChange(optionsLockup[val].label)`), the
        // opposite of Select/TagsInput. So `label` is set to the code
        // itself here, and the friendlier "code — name" text is rendered
        // dropdown-only via `renderOption`, which doesn't affect what
        // gets written into the field.
        data={suggestions.map((s) => ({ value: s.code, label: s.code }))}
        renderOption={({ option }) => {
          const match = suggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      <Button onClick={handleSearch} disabled={crs.trim().length === 0}>
        Look up
      </Button>
    </Group>
  );
}
