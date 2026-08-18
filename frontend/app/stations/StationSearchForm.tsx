'use client';

import { useState, useTransition } from 'react';
import { useRouter } from 'next/navigation';
import { Autocomplete, Button, Group, Skeleton, Stack } from '@mantine/core';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

export function StationSearchForm() {
  const router = useRouter();
  const [crs, setCrs] = useState('');
  const { suggestions } = useSuggestions(crs, searchStations);
  const [isPending, startTransition] = useTransition();

  function handleSearch() {
    const trimmed = crs.trim().toUpperCase();
    if (!trimmed) return;
    // The target `/stations/[crs]` route has no `loading.tsx` of its own,
    // so without this, `isPending` (and therefore all user feedback while
    // its `StopPoint/.../Disruption` fetch — several seconds on the real
    // API — resolves) would never surface. Wrapping the navigation in a
    // transition works because `router.push` itself dispatches through
    // its own nested `startTransition` internally, which keeps ours
    // pending for exactly as long as that dispatch takes to settle.
    startTransition(() => {
      router.push(`/stations/${trimmed}`);
    });
  }

  return (
    <Stack gap="md">
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
        <Button onClick={handleSearch} disabled={isPending || crs.trim().length === 0}>
          {isPending ? 'Looking up…' : 'Look up'}
        </Button>
      </Group>
      {isPending && (
        // Several seconds of a static, disabled button is not enough
        // feedback for where the user is actually looking — this mirrors
        // the shape of the results the target page is about to render.
        <Stack gap="xs" role="status" aria-label="Looking up disruptions">
          <Skeleton height={20} width="40%" />
          <Skeleton height={60} />
        </Stack>
      )}
    </Stack>
  );
}
