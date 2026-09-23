'use client';

import { useState } from 'react';
import Link from 'next/link';
import { Alert, Button, Card, Group, SegmentedControl, Stack, Text } from '@mantine/core';
import { AddJourneyLegButton } from './AddJourneyLegButton';
import { PlanTripFlow } from './PlanTripFlow';
import { TrackTrainForm } from './TrackTrainForm';
import { journeyCanAddLeg, journeyPriorDestinationCrs } from '@/lib/journeyLegChaining';
import { routeLabel } from '@/lib/stationLabel';
import type { CreateJourneyResponse, JourneyDetail } from '@/lib/types';

/** The `/journeys/new` page's own interactive body -- see that page's doc
 * comment for the design this component is the engine of. Two states:
 *
 * 1. `journeyId === null` -- nothing exists yet. Renders `TrackTrainForm`
 *    completely unmodified except for one new prop (`onCreated`), which
 *    this component is the first caller of: instead of `TrackTrainForm`'s
 *    own default `router.push('/journeys/{id}')`, creating leg 1 hands the
 *    result straight back here, and this component stays mounted.
 * 2. `journeyId !== null` -- a real journey now exists (a genuine
 *    `POST /Journeys` row, not a client-side draft). This component
 *    fetches its current state (`GET /api/Journeys/{id}`, the same
 *    same-origin proxy `AddJourneyLegButton` already calls) and renders a
 *    short per-leg summary, an inline "Add a leg" (via `AddJourneyLegButton`,
 *    also unmodified except for the same `onAdded`-instead-of-
 *    `router.refresh()` swap) gated by the exact same `journeyCanAddLeg`
 *    rule the real journey detail page enforces, and a "Done" link to
 *    `/journeys/{id}` that is ALWAYS available regardless of that gate --
 *    a single matched or still-searching leg is already a complete, useful
 *    journey (the whole app already treats a 1-leg journey as first-class),
 *    so finishing after leg 1 must never be blocked on anything.
 *
 * The per-leg summary here is deliberately lighter than the real detail
 * page's `JourneyLegCard` (no candidate picker, no "Change train" toggle,
 * no remove-leg control): this component's job is CREATING the journey,
 * not managing it, and every one of those actions is one click away, via
 * the always-present Done link, on `/journeys/{id}` -- which already has
 * all of them. Duplicating that machinery here would be scope creep for
 * this component and a second place for it to go stale.
 *
 * A failed refetch (`journeyLoadError`) is treated as a display-only
 * gap, never as a reason to block the Done link: the journey itself was
 * already created successfully server-side by the time this component
 * could even attempt the refetch, so a transient network blip here must
 * not strand the user with no way forward. "Add a leg" DOES stay hidden
 * in that case -- `journeyCanAddLeg` has nothing honest to evaluate
 * without a fetched `JourneyDetail` -- but a Retry button offers a way
 * out of that narrower gap without losing the journey itself. */
export function JourneyCreationFlow() {
  const [journeyId, setJourneyId] = useState<number | null>(null);
  const [journey, setJourney] = useState<JourneyDetail | null>(null);
  const [loadingJourney, setLoadingJourney] = useState(false);
  const [journeyLoadError, setJourneyLoadError] = useState(false);
  const [entryMode, setEntryMode] = useState<'known' | 'plan'>('known');

  async function refreshJourney(id: number) {
    setLoadingJourney(true);
    setJourneyLoadError(false);
    try {
      const response = await fetch(`/api/Journeys/${id}`);
      if (!response.ok) {
        setJourneyLoadError(true);
        return;
      }
      const data: JourneyDetail = await response.json();
      setJourney(data);
    } catch {
      setJourneyLoadError(true);
    } finally {
      setLoadingJourney(false);
    }
  }

  function handleLegOneCreated(result: CreateJourneyResponse) {
    setJourneyId(result.journeyId);
    void refreshJourney(result.journeyId);
  }

  function handleLegAdded() {
    if (journeyId !== null) void refreshJourney(journeyId);
  }

  if (journeyId === null) {
    return (
      <Stack gap="md">
        <SegmentedControl
          value={entryMode}
          onChange={value => setEntryMode(value as 'known' | 'plan')}
          data={[
            { label: 'I know my route', value: 'known' },
            { label: 'Plan a route for me', value: 'plan' },
          ]}
        />
        {entryMode === 'known' ? (
          <TrackTrainForm onCreated={handleLegOneCreated} />
        ) : (
          <PlanTripFlow onCreated={handleLegOneCreated} />
        )}
      </Stack>
    );
  }

  const priorDestinationCrs = journey ? journeyPriorDestinationCrs(journey) : null;
  const canAddLeg = journey !== null && journeyCanAddLeg(journey);

  return (
    <Stack gap="md">
      <Alert color="green" title="First leg tracked">
        Add another leg now if this trip involves a change of trains, or finish here — a single leg is already a
        complete, useful journey.
      </Alert>
      {loadingJourney && journey === null && (
        <Text c="dimmed" size="sm">
          Loading your journey…
        </Text>
      )}
      {journeyLoadError && (
        <Alert color="red" title="Couldn't load the journey's latest status">
          <Stack gap="xs">
            <Text size="sm">You can still finish and manage it from the journey page.</Text>
            <Button variant="subtle" size="xs" onClick={() => void refreshJourney(journeyId)}>
              Retry
            </Button>
          </Stack>
        </Alert>
      )}
      {journey && (
        <Stack gap="xs">
          {journey.legs.map((leg, index) => (
            <Card withBorder key={leg.id}>
              <Group justify="space-between" wrap="wrap">
                <Text size="sm">
                  Leg {index + 1}: {routeLabel(leg.originCrs, leg.originName, leg.destinationCrs, leg.destinationName)}
                </Text>
                <Text size="sm" c="dimmed">
                  {leg.trackedTrainState ? 'Train matched' : 'Not yet matched'}
                </Text>
              </Group>
            </Card>
          ))}
        </Stack>
      )}
      <Group>
        {canAddLeg && (
          <AddJourneyLegButton journeyId={journeyId} priorDestinationCrs={priorDestinationCrs} onAdded={handleLegAdded} />
        )}
        <Button component={Link} href={`/journeys/${journeyId}`}>
          Done — view journey
        </Button>
      </Group>
    </Stack>
  );
}
