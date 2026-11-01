'use client';

import { useEffect, useId, useState } from 'react';
import Link from 'next/link';
import { Alert, Button, Card, Group, SegmentedControl, Stack, Text, VisuallyHidden } from '@mantine/core';
import { AddJourneyLegButton } from './AddJourneyLegButton';
import { PlanTripFlow } from './PlanTripFlow';
import { TrackTrainForm } from './TrackTrainForm';
import { UntrackedLegStartForm, type UntrackedLegOrigin } from './UntrackedLegStartForm';
import {
  journeyCanAddLeg,
  journeyPriorArrival,
  journeyPriorDestination,
  journeyPriorDestinationCrs,
  journeyPriorDestinationName,
} from '@/lib/journeyLegChaining';
import { isUntrackedLeg, untrackedLegPlaces, untrackedLegTitle } from '@/lib/untrackedLeg';
import { routeLabel } from '@/lib/stationLabel';
import type { CreateJourneyResponse, JourneyDetail, PlannerOperatorCatalogue, TrainSearchDates } from '@/lib/types';
import type { PlanFormInitial } from '@/lib/tripPlanUrl';
import {
  DEFAULT_JOURNEY_ENTRY_MODE,
  entryModeSearch,
  type JourneyEntryMode,
  type KnownRouteMode,
} from '@/lib/journeyEntryMode';
import { TextLink } from '@/components/TextLink';
import { trackFeature } from '@/lib/usageAnalytics';
import { LoadingText } from '@/components/LoadingText';

/** The body of `/plan`, "Plan a journey" (it was `/journeys/new`'s until
 * DS-194 merged that page into this one) -- see that page's doc comment
 * for the design this component is the engine of. Two states:
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
 * out of that narrower gap without losing the journey itself.
 *
 * `planOrigin` pre-fills the planner's From field, for `/plan?origin=CRS`
 * links such as a station page's "Plan a journey from here", and
 * `planQuery` restores a whole search from the URL. The planner writes each
 * search back into the address bar (`syncUrl`), so a planned route can be
 * shared or bookmarked.
 *
 * `initialEntryMode` is the option selected on first render: the planner
 * unless the page's `?mode=` named another (`lib/journeyEntryMode.ts`).
 * An `untrackedOrigin` overrides it with "Tube, bus or walk".
 * `knownRouteMode` opens "I know my route" on `TrackTrainForm`'s pick or
 * window half. Switching option writes `?mode=` into the address bar
 * (`history.replaceState`, no navigation), so a shared link opens on the
 * same option. Under the options, links start a journey the other ways
 * the page has no form for: a train search, a ticket, a journey template. */
export function JourneyCreationFlow({
  initialEntryMode = DEFAULT_JOURNEY_ENTRY_MODE,
  knownRouteMode,
  planOrigin,
  planQuery,
  planSearchDates,
  planOperators,
  untrackedOrigin,
}: {
  initialEntryMode?: JourneyEntryMode;
  knownRouteMode?: KnownRouteMode | undefined;
  planOrigin?: string | undefined;
  /** The search the page's URL restores (`lib/tripPlanUrl.ts`). */
  planQuery?: PlanFormInitial | undefined;
  /** The planner's date range (`GET /public/trains/search/dates`), read by
   * the page on the server. */
  planSearchDates?: TrainSearchDates | null | undefined;
  /** The planner's operator choices (`GET /Trips/operators`), read by the
   * page on the server; `null` when the read failed. */
  planOperators?: PlannerOperatorCatalogue | null | undefined;
  /** `/plan?fromTflStop=...` (a TfL stop page's "Add as a journey leg"):
   * opens on the "Tube, bus or walk" form, starting at that stop. */
  untrackedOrigin?: UntrackedLegOrigin | null | undefined;
} = {}) {
  const [journeyId, setJourneyId] = useState<number | null>(null);
  const [journey, setJourney] = useState<JourneyDetail | null>(null);
  const [loadingJourney, setLoadingJourney] = useState(false);
  const [journeyLoadError, setJourneyLoadError] = useState(false);
  const [entryMode, setEntryMode] = useState<JourneyEntryMode>(untrackedOrigin ? 'untracked' : initialEntryMode);
  const entryModeLabelId = useId();

  // Usage counts (DS-190): which way in is shown, on arrival and on each
  // switch, until the journey's first leg exists.
  useEffect(() => {
    if (journeyId === null) trackFeature('journey_start', entryMode);
  }, [entryMode, journeyId]);

  async function refreshJourney(id: number) {
    setLoadingJourney(true);
    setJourneyLoadError(false);
    try {
      const response = await fetch(`/api/Journeys/${id}`);
      if (!response.ok) {
        setJourneyLoadError(true);
        return;
      }
      const data = (await response.json()) as JourneyDetail;
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

  function changeEntryMode(value: JourneyEntryMode) {
    setEntryMode(value);
    const search = entryModeSearch(window.location.search, value);
    window.history.replaceState(null, '', `${window.location.pathname}${search}`);
  }

  if (journeyId === null) {
    return (
      <Stack gap="md">
        {/* I3: mirrors `TrackTrainForm.tsx`'s own `modeLabelId` +
            `aria-labelledby` fix (2026-09-22 UX review) for its own mode
            toggle -- without a visible `Text` label wired as the name, a
            screen reader announces an unnamed "radiogroup". This toggle's
            swap is arguably more significant than that one: it swaps the
            ENTIRE form beneath it (`TrackTrainForm` for `PlanTripFlow`, or
            back), not just a sub-section of one form. */}
        <Text id={entryModeLabelId} size="xs" fw={600} c="dimmed">
          How do you want to start this journey?
        </Text>
        {/* Three options don't fit side by side on a phone: Mantine keeps
            each label on one line, so at 390px the third ran past the
            control's edge and was cut off.
            `.ds-entry-mode-control` (globals.css) stacks them below `xs`,
            as `AddJourneyLegButton`'s dialog always does, and lets a label
            wrap rather than clip at any width. No item borders: Mantine
            draws them for the orientation it was given, not the one the
            CSS shows. */}
        <SegmentedControl
          className="ds-entry-mode-control"
          aria-labelledby={entryModeLabelId}
          value={entryMode}
          onChange={(value) => changeEntryMode(value)}
          withItemsBorders={false}
          data={[
            // The default first (`DEFAULT_JOURNEY_ENTRY_MODE`).
            { label: 'Plan a route for me', value: 'plan' },
            { label: 'I know my route', value: 'known' },
            // Spec 2026-10-20 §2.3: a trip that starts away from the
            // railway (a Tube ride or a walk to the station).
            { label: 'Tube, bus or walk', value: 'untracked' },
          ]}
        />
        {/* Same `VisuallyHidden`/`aria-live="polite"` swap announcement as
            `TrackTrainForm.tsx`'s own mode toggle -- sighted users see the
            whole form change, screen-reader users otherwise get nothing
            until they tab forward into different fields. */}
        <VisuallyHidden role="status" aria-live="polite">
          {entryMode === 'plan'
            ? 'Showing the plan-a-route-for-me form.'
            : entryMode === 'untracked'
              ? 'Showing the Tube, bus or walk form.'
              : 'Showing the I-know-my-route form.'}
        </VisuallyHidden>
        {entryMode === 'known' && (
          <TrackTrainForm
            onCreated={handleLegOneCreated}
            {...(knownRouteMode === undefined ? {} : { initialMode: knownRouteMode })}
          />
        )}
        {entryMode === 'plan' && (
          <PlanTripFlow
            onCreated={handleLegOneCreated}
            initialOriginCrs={planOrigin}
            initialQuery={planQuery}
            searchDates={planSearchDates}
            operatorCatalogue={planOperators}
            syncUrl
          />
        )}
        {entryMode === 'untracked' && (
          <UntrackedLegStartForm onCreated={handleLegOneCreated} origin={untrackedOrigin} />
        )}
        {/* The ways in that have a page of their own rather than a form
            here. Shown to everyone: each page asks a logged-out visitor to
            log in itself. */}
        <Group gap="md" align="center">
          <Text size="sm" c="dimmed">
            Other ways to start:
          </Text>
          <TextLink href="/trains">Find a train</TextLink>
          <TextLink href="/track/mine/add-ticket">Add a ticket</TextLink>
          <TextLink href="/journeys/templates">Journey templates</TextLink>
        </Group>
      </Stack>
    );
  }

  const priorDestinationCrs = journey ? journeyPriorDestinationCrs(journey) : null;
  const priorArrival = journey ? journeyPriorArrival(journey) : null;
  const priorPlaceName = journey ? journeyPriorDestinationName(journey) : null;
  const priorStopId = journey ? (journeyPriorDestination(journey)?.stopId ?? null) : null;
  const canAddLeg = journey !== null && journeyCanAddLeg(journey);

  return (
    <Stack gap="md">
      <Alert color="green" title="First leg tracked">
        Add another leg now if this trip involves a change of trains, or finish here. A single leg is already a
        complete, useful journey.
      </Alert>
      {loadingJourney && journey === null && <LoadingText>Loading your journey</LoadingText>}
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
                  Leg {index + 1}:{' '}
                  {isUntrackedLeg(leg)
                    ? [
                        untrackedLegTitle(leg),
                        untrackedLegPlaces(leg, { previous: journey.legs[index - 1], next: journey.legs[index + 1] }),
                      ]
                        .filter(Boolean)
                        .join(', ')
                    : routeLabel(leg.originCrs, leg.originName, leg.destinationCrs, leg.destinationName)}
                </Text>
                <Text size="sm" c="dimmed">
                  {isUntrackedLeg(leg)
                    ? 'Not live tracked'
                    : leg.trackedTrainState
                      ? 'Train matched'
                      : 'Not yet matched'}
                </Text>
              </Group>
            </Card>
          ))}
        </Stack>
      )}
      <Group>
        {canAddLeg && (
          <AddJourneyLegButton
            journeyId={journeyId}
            priorDestinationCrs={priorDestinationCrs}
            priorStopId={priorStopId}
            priorArrival={priorArrival}
            priorPlaceName={priorPlaceName}
            onAdded={handleLegAdded}
          />
        )}
        <Button component={Link} href={`/journeys/${journeyId}`}>
          View journey
        </Button>
      </Group>
    </Stack>
  );
}
