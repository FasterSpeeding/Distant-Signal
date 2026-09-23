'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Group, Stack, Text } from '@mantine/core';
import { TrainJourney } from './TrainJourney';
import { JourneyLegCandidates } from './JourneyLegCandidates';
import { TextLink } from './TextLink';
import { RemoveJourneyLegButton } from './RemoveJourneyLegButton';
import { legRouteAndTime } from '@/lib/journeyLegLabel';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyLegDetail } from '@/lib/types';

/** Decorative "needs attention" glyph for the open-leg card (review
 * §2.3/I15). `@tabler/icons-react` isn't a project dependency (see
 * `InfoIcon.tsx`'s own note) -- inline SVG in the same house style: 16px,
 * `currentColor`, `aria-hidden` (the card's own text already says "pick a
 * train"/"waiting for the owner", so this adds no information a screen
 * reader needs). */
function AlertIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="8" x2="12" y2="12" />
      <line x1="12" y1="16" x2="12.01" y2="16" />
    </svg>
  );
}

/** `"HH:MM:SS"` -> `"HH:MM"` -- these are bare wall-clock bounds
 * (`common::TimeWindow` on the wire), never full RFC3339 instants, so
 * there is no timezone conversion to do (unlike `lib/dateFormat.ts`'s
 * formatters, which all pin `Europe/London` for a real instant) -- just
 * trimming the seconds a passenger never entered. */
function formatWindowTime(value: string): string {
  return value.slice(0, 5);
}

/** "Departing at or after 18:00 · Arriving at or before 09:00"-shaped
 * summary of an open leg's own search window -- 2026-09-22 UX review
 * finding I18: "the window the user just typed is never shown back to
 * them" (spec §4 explicitly asks the open-leg card to show "the search
 * parameters (origin, destination, windows)"; only origin/destination
 * were ever rendered). Phrased to match `AddJourneyLegButton.tsx`'s own
 * field descriptions exactly ("Only trains departing at or after this
 * time.", etc.) rather than inventing new wording for the same facts.
 * `null` when the leg has no window at all (a `pin`/`knownTrain`-mode
 * leg is never `trackedTrainState === null` in the first place, so this
 * realistically never returns `null` at this call site, but it's a
 * plain function of the four fields, not leg-mode-aware, so it stays
 * honest for any future caller). */
/** One side's (depart or arrive) contribution to `windowSummary`, given
 * that side's own after/before bounds. Collapses to a single "between X
 * and Y" phrase when BOTH bounds on this side are set, rather than the
 * literal "departing at or after 19:00 · departing at or before 21:00"
 * the naive per-field join used to produce -- the same verb repeated back
 * to back reads as a copy-paste bug even though it isn't one. `null` when
 * neither bound on this side is set. */
function windowSideSummary(verb: 'departing' | 'arriving', after: string | null, before: string | null): string | null {
  if (after && before) return `${verb} between ${formatWindowTime(after)} and ${formatWindowTime(before)}`;
  if (after) return `${verb} at or after ${formatWindowTime(after)}`;
  if (before) return `${verb} at or before ${formatWindowTime(before)}`;
  return null;
}

function windowSummary(leg: JourneyLegDetail): string | null {
  const parts = [
    windowSideSummary('departing', leg.departAfter, leg.departBefore),
    windowSideSummary('arriving', leg.arriveAfter, leg.arriveBefore),
  ].filter((part): part is string => part !== null);
  if (parts.length === 0) return null;
  const [first, ...rest] = parts;
  const capitalised = first.charAt(0).toUpperCase() + first.slice(1);
  return [capitalised, ...rest].join(' · ');
}

/** One leg's card on `/journeys/[id]` (design doc §4). Two branches:
 *
 * - **Open** (`trackedTrainState === null`): the search parameters plus,
 *   for the journey's OWNER only, a `JourneyLegCandidates` picker -- a
 *   non-owning group member (reachable here via `journey_readable_by`'s
 *   group-shared read path, see this plan's I1 final-review finding) sees
 *   a plain "waiting for the owner" message instead, since picking a train
 *   for someone else's leg is a 404 the backend correctly refuses.
 * - **Open** (`trackedTrainState === null`): the search parameters plus,
 *   for the journey's OWNER only, a "Remove leg" action (top-right of the
 *   card, same convention as the matched branch below) and a
 *   `JourneyLegCandidates` picker -- a non-owning group member (reachable
 *   here via `journey_readable_by`'s group-shared read path, see this
 *   plan's I1 final-review finding) sees a plain "waiting for the owner"
 *   message instead of the picker, since picking a train for someone
 *   else's leg is a 404 the backend correctly refuses; it never sees
 *   "Remove leg" either, for the same reason.
 * - **Matched**: `TrainJourney` (reused unmodified, per the design doc's
 *   own explicit direction) plus, for the journey's OWNER only, up to two
 *   INDEPENDENT actions, not a mutually-exclusive either/or: "Remove leg"
 *   is always offered (backed by `DELETE
 *   /Journeys/{journeyId}/legs/{legId}`,
 *   `crates/api/src/routes/journeys.rs::delete_journey_leg` -- that route
 *   has never cared what created the leg or whether it carries a window,
 *   so gating the BUTTON on `hasWindow` was a frontend-only restriction
 *   with no backend reason behind it), and "Change train" is offered
 *   ADDITIONALLY whenever the leg has a persisted window
 *   (`departAfter`/`departBefore`/`arriveAfter`/`arriveBefore` -- any
 *   non-null), revealing the SAME `JourneyLegCandidates` component,
 *   re-scoped to this leg (its own persisted window drives what the
 *   backend searches -- see
 *   `crates/api/src/routes/journeys.rs::get_leg_candidates`).
 *
 *   Before this fix, the two actions were rendered as a ternary
 *   (`hasWindow ? <ChangeTrain> : <RemoveLeg>`): a leg with NO window got
 *   "Remove leg" but no way to swap its train, and -- the common case,
 *   since most legs are created via the time-window search flow -- a leg
 *   WITH a window got "Change train" but no way to remove it outright,
 *   leaving most legs on most journeys with no delete affordance at all.
 *   Both actions are now independently gated on their own real
 *   precondition (owner; `hasWindow` for "Change train" only), never on
 *   each other.
 *
 * Every action lives in the card's OWN title row, top-right -- 2026-09-22
 * UX review finding I14/2.4's own recommendation: "actions live at the
 * top-right of the thing they act on", the same rule `page.tsx`'s header
 * already applies to "Add a leg". */
export function JourneyLegCard({
  journeyId,
  leg,
  isOwner,
  isOnlyLeg,
}: {
  journeyId: number;
  leg: JourneyLegDetail;
  isOwner: boolean;
  /** Whether this is the journey's only remaining leg -- threaded from
   * `page.tsx`'s own `journey.legs.length`, and passed straight through to
   * `RemoveJourneyLegButton` so it can warn (and, on success, redirect
   * instead of refresh) when removing this leg also removes the whole
   * journey. See that component's own doc comment. */
  isOnlyLeg: boolean;
}) {
  const router = useRouter();
  const [changingTrain, setChangingTrain] = useState(false);
  const hasWindow =
    leg.departAfter !== null ||
    leg.departBefore !== null ||
    leg.arriveAfter !== null ||
    leg.arriveBefore !== null;

  if (leg.trackedTrainState === null) {
    // Review §2.5/I27 + review finding 2.9 (the two branches reached the
    // same conclusion from opposite ends): this used to interpolate raw
    // CRS codes and a raw ISO `serviceDate` ("YRK -> NCL, 2026-09-22")
    // while the matched card one row down rendered "London Kings Cross
    // (KGX) -> York (YRK), 22 Sept 2026" -- two date formats and two
    // naming conventions on the same page. `routeLabel`/`formatDate` are
    // the shared formatters both `TrainJourney` and `/track/mine` already
    // use. Unlike the matched card there is no `journeyStops` to read
    // names from, but the leg itself now carries `originName`/
    // `destinationName` (resolved server-side from the `stations`
    // reference table), so this degrades to bare codes only when the
    // reference row genuinely doesn't exist.
    const header = `${routeLabel(leg.originCrs, leg.originName, leg.destinationCrs, leg.destinationName)}, ${formatDate(leg.serviceDate)}`;
    // Review I18 (found independently by two reviewers): the window the
    // traveller just typed was never shown back to them -- the only place
    // it lived was inside `hasWindow`'s own boolean check below, never
    // rendered. The copy deliberately mirrors `AddJourneyLegButton.tsx`'s
    // own field descriptions ("Only trains departing at or after this
    // time.") rather than inventing new wording for the same facts, and
    // says "at or after" rather than "after" because that is what the
    // bound actually means. It does NOT repeat the station names: the
    // header directly above already carries the route.
    const criteria = windowSummary(leg);
    return (
      // Review §2.3/I15: the matched card (below) is dense -- headcode,
      // route, summary lines, a badge, a diagram, a table -- while THIS
      // card, the one that actually needs the user to do something, used
      // to be a plain white box with nothing in its border, background,
      // weight or icon saying "action required". Four non-colour-alone
      // cues (WCAG 1.4.1): a left accent border, a tinted surface, a bold
      // title, and a leading glyph -- all in the blue the app already
      // reserves for "needs attention" (`JourneyStatusBadge`'s own
      // `unmatched` colour).
      <Card
        withBorder
        style={{
          borderLeftWidth: 4,
          borderLeftColor: 'var(--mantine-color-blue-6)',
          backgroundColor: 'var(--mantine-color-blue-light)',
        }}
      >
        <Stack gap="sm">
          <Group justify="space-between" align="flex-start" wrap="wrap" gap="xs">
            <Group gap="xs" wrap="nowrap" align="flex-start">
              <span style={{ color: 'var(--mantine-color-blue-6)', flexShrink: 0, marginTop: 2 }}>
                <AlertIcon />
              </span>
              <Text fw={700}>{header}</Text>
            </Group>
            {/* An open leg is just as removable as a matched one -- the
                backend's `delete_journey_leg` never cared whether the leg
                had a train bound yet, only who owns the journey. This was
                previously offered only on the matched branch, leaving an
                open (never-yet-matched) leg with no delete affordance at
                all. */}
            {isOwner && <RemoveJourneyLegButton journeyId={journeyId} legId={leg.id} isOnlyLeg={isOnlyLeg} />}
          </Group>
          {criteria && (
            <Group justify="space-between" wrap="wrap" gap="xs">
              <Text size="sm" c="dimmed">
                {criteria}
              </Text>
              {isOwner && (
                // Review §2.1/I21: `/track` now accepts `?mode=window`, so
                // this finally has somewhere honest to link to -- prefills
                // the origin the same way the station-page "Track a train
                // from here" link already does. Doesn't restore the
                // destination/date/times too (no query-param contract for
                // those yet); a real re-search, not a form round-trip.
                <TextLink href={`/track?mode=window&origin=${encodeURIComponent(leg.originCrs ?? '')}`}>
                  Edit search
                </TextLink>
              )}
            </Group>
          )}
          {isOwner ? (
            <JourneyLegCandidates
              journeyId={journeyId}
              legId={leg.id}
              serviceDate={leg.serviceDate}
              onPicked={() => router.refresh()}
            />
          ) : (
            <Text size="sm" c="dimmed">
              Waiting for the owner to pick a train.
            </Text>
          )}
        </Stack>
      </Card>
    );
  }

  const state = leg.trackedTrainState;

  // Leg-scoped skip signal (§5.2) -- at most the leg's own origin/destination
  // CRS, sourced from Task 4's `legSkip` wire field. `TrainJourney`/
  // `JourneyTimeline` treat `undefined` and `[]` identically, but this is
  // always a concrete (possibly empty) array here since `leg.legSkip` is
  // only `null` when there's nothing to report.
  const skippedCrs = [
    leg.legSkip?.originSkipped ? leg.originCrs : null,
    leg.legSkip?.destinationSkipped ? leg.destinationCrs : null,
  ].filter((crs): crs is string => crs !== null);

  const title = legRouteAndTime(leg.originCrs, leg.destinationCrs, state.journeyStops, state);

  return (
    <Card withBorder>
      <Stack gap="sm">
        <Group justify="space-between" align="flex-start" wrap="wrap" gap="xs">
          <Stack gap={2}>
            <Text fw={600} size="lg">
              {title}
            </Text>
            {state.trainUid && (
              <Text size="xs" c="dimmed">
                Train {state.trainUid}
              </Text>
            )}
          </Stack>
          {/* `isOwner` is Phase 4's sharing gate -- a shared-group viewer
              sees the leg but must not be offered "Change train"/"Remove
              leg", either of which the API would reject anyway. Below
              that, the two actions are INDEPENDENT, not an either/or:
              "Remove leg" is always available to the owner, and "Change
              train" is available ADDITIONALLY whenever `hasWindow` (the
              leg has something to re-search). See this component's own
              doc comment for why this used to be a `hasWindow ? ... : ...`
              ternary, and why that was a real gap. */}
          {isOwner && (
            <Group gap="xs">
              {hasWindow && (
                <Button size="xs" variant="default" onClick={() => setChangingTrain((c) => !c)}>
                  {changingTrain ? 'Cancel' : 'Change train'}
                </Button>
              )}
              <RemoveJourneyLegButton journeyId={journeyId} legId={leg.id} isOnlyLeg={isOnlyLeg} />
            </Group>
          )}
        </Group>
        <TrainJourney
          state={state}
          skippedCrs={skippedCrs}
          legDestinationCrs={leg.destinationCrs}
          suppressTrainUidHeading
        />
        {hasWindow && isOwner && changingTrain && (
          <JourneyLegCandidates
            journeyId={journeyId}
            legId={leg.id}
            serviceDate={leg.serviceDate}
            onPicked={() => {
              setChangingTrain(false);
              router.refresh();
            }}
          />
        )}
      </Stack>
    </Card>
  );
}
