'use client';

import { useState, useTransition } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Skeleton, Stack, Text } from '@mantine/core';
import { TrackEmptyState } from '@/components/TrackEmptyState';
import { useMounted } from '@mantine/hooks';
import { searchNearbyStations } from '@/lib/suggestions';
import { searchStationsAndStops } from '@/lib/stationSearch';
import { tflStopHref } from '@/lib/tflStops';
import type { NearbyStation, Suggestion } from '@/lib/types';
import { LoadingPlaceholder } from '@/components/LoadingPlaceholder';
import { ButtonSpinner, LoadingText } from '@/components/LoadingText';
import { StationField } from '@/components/StationField';
import { NearbyStationList } from './NearbyStationList';

/** The "near me" lookup's own state machine -- mirrors the shape
 * `IncidentSearchForm.tsx`'s `Results` type already establishes (one
 * mutually-exclusive union rather than several independent booleans that
 * could disagree). `'denied'` is kept apart from `'error'` on purpose: a
 * declined location prompt is a common, expected outcome that deserves
 * calm, non-alarming copy, not the same "something went wrong" treatment as
 * a genuine failure (a timeout, a GPS-less device). `'lookupError'` is the
 * location arriving but the station lookup failing: the device did its
 * part, so the copy doesn't blame the location. */
type NearbyState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'success'; stations: NearbyStation[] }
  | { status: 'denied' }
  | { status: 'error' }
  | { status: 'lookupError' };

// `GeolocationPositionError.PERMISSION_DENIED`'s standard numeric value
// (https://developer.mozilla.org/docs/Web/API/GeolocationPositionError) --
// compared as a plain number rather than via the constant on the error
// instance so a plain object (a test double, or any future non-browser
// implementation) works the same way a real `GeolocationPositionError`
// does.
const GEOLOCATION_PERMISSION_DENIED = 1;

export function StationSearchForm() {
  const router = useRouter();
  // The picked code, or the text typed so far (`StationField`).
  const [crs, setCrs] = useState('');
  // The dropdown row last picked, while the field still holds it: "Euston"
  // the Tube stop and "Euston" the station share a name, so the name alone
  // can't say which was meant.
  const [picked, setPicked] = useState<Suggestion | null>(null);
  // The field's live suggestions, for "Look up" on text nothing was picked for.
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
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

  // The "Look up" button's navigation. The "near me" results are plain
  // links instead (`NearbyStationList`), with their own pending hint.
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

  /** Where a search result leads: a station's page, a TfL stop's page
   * (`/tfl/stops/[id]`, live arrivals and line status) or a tram stop's
   * page. */
  function open(suggestion: Suggestion) {
    const place = suggestion.place;
    if (!place) {
      goToStation(suggestion.code);
    } else if (place.kind === 'tfl') {
      const href = tflStopHref(place.stop.id);
      startTransition(() => {
        router.push(href);
      });
    } else {
      startTransition(() => {
        router.push(`/transit/${encodeURIComponent(place.network)}/stops/${encodeURIComponent(place.stopId)}`);
      });
    }
  }

  function handleChange(value: string) {
    setCrs(value);
    // Any other text than the picked row's code is a new search.
    setPicked((current) => (current?.code === value ? current : null));
  }

  function handleSearch() {
    const trimmed = crs.trim();
    if (!trimmed) return;
    if (picked) {
      open(picked);
      return;
    }
    // A typed exact code or name is picked by the field itself (on blur or
    // Enter), so this is other text: the best (first) live match, and only
    // the raw text if nothing matched at all (e.g. a network hiccup).
    const target = suggestions[0];
    if (target) open(target);
    else goToStation(trimmed.toUpperCase());
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
          .catch(() => setNearby({ status: 'lookupError' }));
      },
      (error) => {
        setNearby({ status: error.code === GEOLOCATION_PERMISSION_DENIED ? 'denied' : 'error' });
      },
    );
  }

  return (
    <Stack gap="md">
      <Group align="end">
        <StationField
          label="Station name or code"
          fieldName="Station"
          placeholder="e.g. Woking or WOK"
          value={crs}
          onChange={handleChange}
          onSuggestionPick={setPicked}
          onSuggestionsChange={setSuggestions}
          search={searchStationsAndStops}
        />
        <Button
          onClick={handleSearch}
          disabled={isPending || crs.trim().length === 0}
          leftSection={isPending ? <ButtonSpinner /> : undefined}
        >
          {isPending ? 'Looking up' : 'Look up'}
        </Button>
      </Group>
      {isPending && (
        // Several seconds of a static, disabled button is not enough
        // feedback for where the user is actually looking — this mirrors
        // the shape of the results the target page is about to render.
        // `LoadingPlaceholder` gives it a visible, announced label: the
        // old `aria-label` on a `role="status"` of bare `Skeleton`s gave a
        // live region with no content to announce and nothing in words
        // for a sighted user either.
        <LoadingPlaceholder label="Looking up disruptions">
          <Skeleton height={20} width="40%" />
          <Skeleton height={60} />
        </LoadingPlaceholder>
      )}
      {/* Hidden entirely (not just disabled) when this browser has no
          `navigator.geolocation` at all -- outcome (c) of the feature's own
          spec: a button that can only ever silently fail on click is worse
          than no button. */}
      {geolocationSupported && (
        <Stack gap="xs">
          <Group>
            <Button
              variant="default"
              onClick={handleNearMe}
              disabled={nearby.status === 'loading'}
              leftSection={nearby.status === 'loading' ? <ButtonSpinner /> : undefined}
            >
              {nearby.status === 'loading' ? 'Locating' : 'Use my location'}
            </Button>
          </Group>
          {nearby.status === 'loading' && (
            // A real device/GPS fix can take several seconds -- this is the
            // "the UI isn't frozen" signal for that wait, same `role="status"`
            // convention the pending-navigation Skeleton above and
            // `ConnectivityMonitor`'s reconnecting banner both use.
            <LoadingText role="status" aria-live="polite">
              Finding stations near you
            </LoadingText>
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
          {nearby.status === 'lookupError' && (
            <Alert color="red" title="Couldn't load nearby stations">
              Try again, or search by name above.
            </Alert>
          )}
          {nearby.status === 'success' && nearby.stations.length === 0 && (
            <Text size="sm" c="dimmed" role="status">
              No stations found near you. Search by name above.
              <TrackEmptyState kind="station_search_no_results" />
            </Text>
          )}
          {nearby.status === 'success' && nearby.stations.length > 0 && (
            <NearbyStationList stations={nearby.stations} />
          )}
        </Stack>
      )}
    </Stack>
  );
}
