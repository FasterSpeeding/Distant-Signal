import { Alert, Badge, Group, Loader, Stack, Text, Tooltip } from '@mantine/core';
import { EtaBadge } from './EtaBadge';
import { JourneyProgress } from './JourneyProgress';
import { JourneyTimeline, type JourneyEndpointNames } from './JourneyTimeline';
import { formatTime } from '@/lib/dateFormat';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { TrainJourneyState } from '@/lib/types';

/** Renders one train's journey through every state the backend can
 * return, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 3's original table, revised by
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md:
 * the scheduled timetable (`state.journeyStops`) is now the PRIMARY,
 * always-shown structure whenever it's available (i.e. whenever
 * `trainUid` is known -- `schedule_matched` or any `resolved` sub-state),
 * rendered once at the top level via `JourneyTimeline`, rather than nested
 * inside only the `resolved`+`en_route` branch the way the old
 * `JourneyDetails` denormalized summary was. `StatusMessage` below is the
 * original per-state switch, kept for its status copy/alerts.
 * `JourneyDetails`'s live-summary content (ETA badge, last-reported
 * location, overall delay, next calling point) renders ALONGSIDE
 * `JourneyTimeline`, not instead of it, whenever `resolutionStatus ===
 * 'resolved'` -- per the design doc §4's requirement that the existing
 * top-level `EtaBadge`/"Last reported" summary stay visible even for the
 * common case of a resolved, en_route train that also has `journeyStops`.
 * `JourneyTimeline` renders additionally whenever `journeyStops` is
 * non-null, independent of `JourneyDetails`.
 *
 * `suppressTrainUidHeading` (Task 3.6.9): `app/train/[uid]/[date]/page.tsx`'s
 * own `<h1>` already reads "Train {uid}" verbatim, and several
 * `StatusMessage` branches below print that exact same "Train {trainUid}"
 * line immediately under it -- a real duplicate, not just visually
 * similar text, since `state.trainUid` IS that page's own `uid`. Default
 * `false` keeps every other caller (`app/train/by-id/[trackingId]`, whose
 * own `<h1>` reads "Tracking Train {trackingId}" -- a DIFFERENT
 * identifier, so its "Train {trainUid}" line is new information, not a
 * repeat) rendering exactly as before. */
export function TrainJourney({
  state,
  suppressTrainUidHeading = false,
}: {
  state: TrainJourneyState;
  suppressTrainUidHeading?: boolean;
}) {
  // See `JourneyTimeline.tsx`'s own doc comment on `JourneyEndpointNames`
  // (Task 3.6.2) -- the tracked pin's own origin/destination, always known
  // even when a particular calling point's TIPLOC->CRS->name join didn't
  // resolve. Computed once here and threaded to both `JourneyProgress` and
  // `JourneyTimeline` so the two can never show a different label for the
  // same endpoint stop.
  const endpointNames: JourneyEndpointNames = {
    originName: state.pinOriginName ?? state.pinOriginCrs,
    destinationName: state.pinDestinationName ?? state.pinDestinationCrs,
  };

  return (
    <Stack gap="sm">
      <StatusMessage state={state} suppressTrainUidHeading={suppressTrainUidHeading} />
      {state.resolutionStatus === 'resolved' && <JourneyDetails state={state} />}
      {state.journeyStops && (
        <JourneyProgress
          stops={state.journeyStops}
          resolutionStatus={state.resolutionStatus}
          status={state.status}
          trainUid={state.trainUid}
          mayHaveArrived={state.mayHaveArrived}
          lastReportedLocation={state.lastReportedLocation}
          endpointNames={endpointNames}
        />
      )}
      {state.journeyStops && <JourneyTimeline stops={state.journeyStops} endpointNames={endpointNames} />}
    </Stack>
  );
}

function StatusMessage({
  state,
  suppressTrainUidHeading,
}: {
  state: TrainJourneyState;
  suppressTrainUidHeading: boolean;
}) {
  // See `TrainJourney`'s own doc comment on `suppressTrainUidHeading`.
  // `null` renders nothing either way (`{trainUidLine}` below), so this
  // never hides a genuinely different fact -- it only ever removes an
  // exact repeat of the page's own `<h1>`.
  const trainUidLine = suppressTrainUidHeading ? null : <Text fw={500}>Train {state.trainUid}</Text>;

  const pinSummary = (
    <Text size="sm" c="dimmed">
      {trackedTrainDisplayName(state)}
    </Text>
  );

  if (state.resolutionStatus === 'pending') {
    return (
      <Stack gap="sm" role="status">
        <Group gap="sm">
          <Loader size="sm" />
          <Text fw={500}>Waiting to hear from Network Rail</Text>
        </Group>
        {pinSummary}
        <Text size="sm" c="dimmed">
          This train hasn&apos;t been matched to a live service yet — that&apos;s normal if it hasn&apos;t
          started running. Network Rail typically doesn&apos;t report a service until shortly before it
          departs. This page updates automatically.
        </Text>
      </Stack>
    );
  }

  if (state.resolutionStatus === 'schedule_matched') {
    const destination = state.scheduleDestinationName ?? state.scheduleDestinationCrs;
    return (
      <Stack gap="sm">
        <Group gap="xs">
          <Text fw={500}>
            Matched to a scheduled service — Train {state.trainUid}
            {destination ? ` to ${destination}` : ''}
          </Text>
          <Tooltip label="This is the booked timetable, not a live report yet. It may change if Network Rail issues a late alteration, and we'll update this automatically once live tracking begins.">
            <Badge color="gray" variant="light">
              As scheduled
            </Badge>
          </Tooltip>
        </Group>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Waiting for Network Rail&apos;s live tracking to begin.
        </Text>
      </Stack>
    );
  }

  if (state.resolutionStatus === 'unresolved') {
    return (
      <Stack gap="sm">
        <Text fw={500} c="var(--ds-color-error-text)">
          Couldn&apos;t be matched to a live service
        </Text>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Network Rail never reported a matching service for this pin. This won&apos;t resolve on its own
          — try tracking the train again if it was a genuine mistake.
        </Text>
      </Stack>
    );
  }

  // resolutionStatus === 'resolved' from here on -- trainUid is non-null
  // per the backend's own resolution invariant (a tracked train is only
  // ever set to 'resolved' in the same write that sets train_uid), even
  // though the TypeScript type can't express that correlation across two
  // separate optional fields.
  if (state.status === 'awaiting_activation' || state.status === null) {
    return (
      <Stack gap="sm">
        <Text fw={500}>Matched to train {state.trainUid}</Text>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Waiting for its first movement report.
        </Text>
      </Stack>
    );
  }

  if (state.status === 'cancelled') {
    return (
      <Stack gap="sm">
        <Alert color="red" title="Cancelled">
          This service was cancelled.
        </Alert>
        {trainUidLine}
        {pinSummary}
      </Stack>
    );
  }

  // `'completed'` is a REAL, backend-confirmed status -- an ARRIVAL event
  // Network Rail reported at this train's own final calling point, not an
  // inference. Two backend paths can set it, and they agree on that rule:
  // `trust_schema::journey::apply_movement`'s ingest-time
  // destination-CRS check, and (in practice the more common one, since the
  // first only fires when `trains.destination_crs` already happened to be
  // known as the event arrived) `api::data::journey::apply_confirmed_arrival`,
  // which reads the same fact off the journey timeline at request time,
  // anchored to the FINAL calling point by position -- so a circular
  // service that terminates back at its own origin CRS resolves correctly.
  // It gets its own distinct,
  // positive rendering below, entirely separate from the "may have
  // arrived" heuristic: that banner is deliberately worded as an
  // inference and must never be reachable for a train we actually KNOW
  // has arrived.
  if (state.status === 'completed') {
    const destination = state.scheduleDestinationName ?? state.scheduleDestinationCrs;
    const terminusStop = state.journeyStops?.at(-1) ?? null;
    const arrivalTime = terminusStop?.actualArrival ?? null;
    return (
      <Stack gap="sm">
        <Alert color="green" title="Arrived" variant="light">
          {destination
            ? `This train has arrived at ${destination}${arrivalTime ? `, at ${formatTime(arrivalTime)}` : ''}.`
            : 'This train has arrived at its final destination.'}
        </Alert>
        {trainUidLine}
        {pinSummary}
      </Stack>
    );
  }

  // Only reachable for 'en_route' here -- a genuinely confirmed arrival is
  // handled entirely by the `'completed'` branch above and never reaches
  // this point. `state.mayHaveArrived` is computed server-side
  // (`crates/api/src/data/journey.rs`'s `may_have_arrived`), from whether
  // now is more than 15 minutes past the ESTIMATED arrival at the
  // journey's final calling point -- replacing the old client-only
  // heuristic (`status === 'en_route' && nextCallingPoint === null`),
  // which fired almost always since `nextCallingPoint` is essentially
  // never populated in practice. Still deliberately worded as an
  // inference ("may have"), never asserted as fact.
  const mayHaveArrived = state.mayHaveArrived;

  return (
    <Stack gap="sm">
      {trainUidLine}
      {pinSummary}
      {mayHaveArrived && (
        // Task 3.6.12 asked this Alert to read `--mantine-color-yellow-light`/
        // `-light-color` explicitly so `variant="light"` "survives into
        // dark" -- it already does, unmodified: `color="yellow"
        // variant="light"` is exactly what makes Mantine's own
        // `defaultVariantColorsResolver` resolve to those two custom
        // properties (verified against
        // node_modules/@mantine/core's own resolver, not assumed), in
        // both schemes. Checked the dark scheme's own stock values by
        // hand (`@mantine/core/styles.css`): background
        // `rgba(115,60,0,1)` against text `--mantine-color-yellow-0`
        // (`#fff9db`) is ~8.3:1 by the WCAG relative-luminance formula --
        // comfortably above AA, not the "washed out" case this task
        // otherwise fixes for the four LIGHT-scheme badge/alert pairings
        // in `app/globals.css` (whose own `-light-color` overrides are
        // deliberately scoped to `[data-mantine-color-scheme='light']`
        // only, because dark already inverts to a safe pairing there
        // too). No override added here: doing so without a live
        // re-measurement risked replacing an already-good pairing with a
        // guessed one.
        <Alert color="yellow" title="May have arrived" variant="light">
          This journey may have arrived at its destination, but this is an inference, not a confirmed
          status from Network Rail.
        </Alert>
      )}
    </Stack>
  );
}

/** The existing live-summary content (ETA badge, last-reported location,
 * overall delay, next calling point) -- rendered for every `resolved`
 * state, regardless of whether `journeyStops` is also present, per the
 * design doc §4's requirement that this summary stay visible alongside
 * `JourneyTimeline`, not be superseded by it. This also remains the ONLY
 * rendering for the one gap the design doc's §1 names: a resolved train
 * with no `journeyStops` at all (not itself a CIF-published schedule that
 * day). Unchanged from the pre-restructuring version, minus its own
 * now-redundant "no movement data" early return duplicating what
 * `TrainJourney` above already gates on via `resolutionStatus === 'resolved'`. */
function JourneyDetails({ state }: { state: TrainJourneyState }) {
  const hasMovementData =
    state.lastReportedLocation !== null ||
    state.delayMinutes !== null ||
    state.nextCallingPoint !== null ||
    state.etaNext !== null;

  if (!hasMovementData) {
    return (
      <Stack gap={4}>
        <Text size="sm" c="dimmed">
          No movement data reported yet.
        </Text>
      </Stack>
    );
  }

  return (
    <Stack gap={4}>
      {state.lastReportedLocation && (
        <Text size="sm">
          Last reported: {state.lastReportedLocation}
          {state.lastEventType ? ` (${state.lastEventType.toLowerCase()})` : ''}
        </Text>
      )}
      {state.delayMinutes !== null && (
        <Group gap={6}>
          <Text size="sm">Delay:</Text>
          {/* `tt="none"` (Task 3.6.12): Mantine's default Badge text is
              uppercase at 11px -- review §5.12 measured this specific
              badge's contrast as borderline (~3.5:1) at that size, and
              WCAG's relaxed "large text" 3:1 threshold only applies at
              18.66px bold or larger, nowhere near this badge. Dropping
              the transform doesn't change the underlying hex contrast
              ratio, but it's the cheaper of the two review-sanctioned
              fixes (the other being a size bump) and removes uppercase's
              own separate legibility cost (thinner apparent stroke
              contrast from losing ascenders/descenders) on top of it. */}
          <Badge color={state.delayMinutes > 0 ? 'orange' : 'green'} variant="light" tt="none">
            {state.delayMinutes > 0 ? `${state.delayMinutes}m late` : 'On time'}
          </Badge>
        </Group>
      )}
      {state.nextCallingPoint && <Text size="sm">Next calling point: {state.nextCallingPoint}</Text>}
      {/* `mayHaveArrived`/`destinationCrs`/`destinationName` (Task 3.6.3):
          see `EtaBadge.tsx`'s own doc comment. Same `scheduleDestinationName
          ?? scheduleDestinationCrs` precedence `StatusMessage` already uses
          twice above for "destination" -- not `pinDestination*`, which is
          only what the user originally typed and may not be this train's
          real terminus. */}
      <EtaBadge
        etaNext={state.etaNext}
        etaSource={state.etaSource}
        mayHaveArrived={state.mayHaveArrived}
        destinationCrs={state.scheduleDestinationCrs}
        destinationName={state.scheduleDestinationName}
      />
    </Stack>
  );
}
