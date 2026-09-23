'use client';

import { useState, useTransition } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Autocomplete, Button, Group, Skeleton, Stack, Text, UnstyledButton } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { searchNearbyStations, searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import type { NearbyStation } from '@/lib/types';

/** The "near me" lookup's own state machine -- mirrors the shape
 * `IncidentSearchForm.tsx`'s `Results` type already establishes (one
 * mutually-exclusive union rather than several independent booleans that
 * could disagree). `'denied'` is kept apart from `'error'` on purpose: a
 * declined location prompt is a common, expected outcome that deserves
 * calm, non-alarming copy, not the same "something went wrong" treatment as
 * a genuine failure (a timeout, a GPS-less device). */
type NearbyState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; stations: NearbyStation[] }
  | { status: 'denied' }
  | { status: 'error' };

// `GeolocationPositionError.PERMISSION_DENIED`'s standard numeric value
// (https://developer.mozilla.org/docs/Web/API/GeolocationPositionError) --
// compared as a plain number rather than via the constant on the error
// instance so a plain object (a test double, or any future non-browser
// implementation) works the same way a real `GeolocationPositionError`
// does.
const GEOLOCATION_PERMISSION_DENIED = 1;

export function StationSearchForm() {
  const router = useRouter();
  const [crs, setCrs] = useState('');
  const { suggestions, loading } = useSuggestions(crs, searchStations);
  const [isPending, startTransition] = useTransition();
  const [nearby, setNearby] = useState<NearbyState>({ status: 'idle' });
  // `navigator.geolocation` doesn't exist during SSR (no `navigator` at
  // all in that environment) and, even client-side, must never be touched
  // before hydration -- same pre-mount discipline `ConnectivityMonitor`/
  // `ThemeToggle`/`PrideToggle` already use for other browser-only APIs.
  // Gating the button's very presence on `mounted` (rather than just
  // gating the click handler) is also what satisfies outcome (c) in the
  // feature's own spec: a browser with no `navigator.geolocation` at all
  // must never show a button that would silently fail when pressed.
  const mounted = useMounted();
  const geolocationSupported = mounted && typeof navigator !== 'undefined' && 'geolocation' in navigator;

  // Shared by the "Look up" button and every "near me" result -- both are
  // just "navigate to this station's page with the same pending-state UX".
  function goToStation(target: string) {
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
    goToStation(target);
  }

  // Only ever invoked from the "Use my location" button's own `onClick` --
  // never from an effect that could fire the location prompt unasked for
  // on mount, which would be bad UX regardless of the SSR-safety reason
  // above.
  function handleNearMe() {
    if (!geolocationSupported) return;
    setNearby({ status: 'loading' });
    navigator.geolocation.getCurrentPosition(
      (position) => {
        searchNearbyStations(position.coords.latitude, position.coords.longitude)
          .then((stations) => setNearby({ status: 'success', stations }))
          .catch(() => setNearby({ status: 'error' }));
      },
      (error) => {
        setNearby({ status: error.code === GEOLOCATION_PERMISSION_DENIED ? 'denied' : 'error' });
      },
    );
  }

  return (
    <Stack gap="md">
      <Group align="end">
        <Autocomplete
          label="Station name or CRS code"
          placeholder="e.g. Woking or WOK"
          value={crs}
          onChange={setCrs}
          {...suggestionAutocompleteProps(suggestions, {
            query: crs,
            loading,
            noMatchMessage: 'No matching stations',
          })}
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
      {/* Hidden entirely (not just disabled) when this browser has no
          `navigator.geolocation` at all -- outcome (c) of the feature's own
          spec: a button that can only ever silently fail on click is worse
          than no button. */}
      {geolocationSupported && (
        <Stack gap="xs">
          <Group>
            <Button variant="default" onClick={handleNearMe} disabled={nearby.status === 'loading'}>
              {nearby.status === 'loading' ? 'Locating…' : 'Use my location'}
            </Button>
          </Group>
          {nearby.status === 'loading' && (
            // A real device/GPS fix can take several seconds -- this is the
            // "the UI isn't frozen" signal for that wait, same `role="status"`
            // convention the pending-navigation Skeleton above and
            // `ConnectivityMonitor`'s reconnecting banner both use.
            <Text size="sm" c="dimmed" role="status" aria-live="polite">
              Finding stations near you…
            </Text>
          )}
          {nearby.status === 'denied' && (
            // A declined location prompt is a common, expected outcome --
            // not an error state -- so this is calm, non-alarming copy in
            // the same dimmed `Text` register as an ordinary empty state,
            // not a red `Alert`.
            <Text size="sm" c="dimmed" role="status" aria-live="polite">
              Location access was declined. You can still search by name above.
            </Text>
          )}
          {nearby.status === 'error' && (
            <Alert color="red" title="Couldn't find your location">
              Try again, or search by name above.
            </Alert>
          )}
          {nearby.status === 'success' && nearby.stations.length === 0 && (
            <Text size="sm" c="dimmed" role="status">
              No stations with known coordinates were found near you.
            </Text>
          )}
          {nearby.status === 'success' && nearby.stations.length > 0 && (
            <Stack
              component="ul"
              gap={4}
              role="list"
              aria-label="Nearby stations"
              style={{ listStyle: 'none', margin: 0, padding: 0 }}
            >
              {nearby.stations.map((station) => (
                <li key={station.code}>
                  <UnstyledButton onClick={() => goToStation(station.code)} disabled={isPending}>
                    {station.name} ({station.code}) — {station.distanceKm.toFixed(1)} km
                  </UnstyledButton>
                </li>
              ))}
            </Stack>
          )}
        </Stack>
      )}
    </Stack>
  );
}
