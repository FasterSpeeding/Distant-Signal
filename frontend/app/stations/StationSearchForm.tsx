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
    const trimmed = crs.trim();
    if (!trimmed) return;
    // Clicking "Look up" (rather than picking a dropdown option) used to
    // navigate using the raw typed text uppercased, as if it were always
    // already a CRS code -- so a typed station name only ever worked by
    // accident. Resolve against the live suggestions the same way
    // selecting from the dropdown would: an exact code or name match
    // first, then the best (first) substring match, and only fall back to
    // the raw text if nothing matched at all (e.g. a network hiccup).
    const exactCode = suggestions.find((s) => s.code.toLowerCase() === trimmed.toLowerCase());
    const exactName = suggestions.find((s) => s.name.toLowerCase() === trimmed.toLowerCase());
    const target = exactCode?.code ?? exactName?.code ?? suggestions[0]?.code ?? trimmed.toUpperCase();
    // The target `/stations/[crs]` route has no `loading.tsx` of its own,
    // so without this, `isPending` (and therefore all user feedback while
    // its `StopPoint/.../Disruption` fetch — several seconds on the real
    // API — resolves) would never surface. Wrapping the navigation in a
    // transition works because `router.push` itself dispatches through
    // its own nested `startTransition` internally, which keeps ours
    // pending for exactly as long as that dispatch takes to settle.
    startTransition(() => {
      router.push(`/stations/${target}`);
    });
  }

  return (
    <Stack gap="md">
      <Group align="end">
        <Autocomplete
          label="Station name or CRS code"
          placeholder="e.g. Woking or WOK"
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
          // `suggestions` is already server-side filtered (the API matches
          // the search term against both CRS code and station name), so
          // Mantine's default client-side re-filtering -- which only checks
          // `label` (the code) -- would hide correct matches when the user
          // searched by station name instead of code. Disable it: show
          // whatever `suggestions` already contains, unfiltered further.
          filter={({ options }) => options}
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
