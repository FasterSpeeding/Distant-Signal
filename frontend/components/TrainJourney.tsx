import { Alert, Badge, Group, Loader, Stack, Text, Tooltip } from '@mantine/core';
import { EtaBadge } from './EtaBadge';
import { JourneyTimeline } from './JourneyTimeline';
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
 * non-null, independent of `JourneyDetails`. */
export function TrainJourney({ state }: { state: TrainJourneyState }) {
  return (
    <Stack gap="sm">
      <StatusMessage state={state} />
      {state.resolutionStatus === 'resolved' && <JourneyDetails state={state} />}
      {state.journeyStops && <JourneyTimeline stops={state.journeyStops} />}
    </Stack>
  );
}

function StatusMessage({ state }: { state: TrainJourneyState }) {
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
        <Text fw={500} c="red">
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
        <Text fw={500}>Train {state.trainUid}</Text>
        {pinSummary}
      </Stack>
    );
  }

  // `'completed'` is now a REAL, backend-confirmed status
  // (`trust_schema::journey::apply_movement`'s destination-CRS check) --
  // an ARRIVAL event Network Rail reported at this train's own known
  // final calling point, not an inference. It gets its own distinct,
  // positive rendering below, entirely separate from the "may have
  // finished" heuristic: that banner is deliberately worded as an
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
        <Text fw={500}>Train {state.trainUid}</Text>
        {pinSummary}
      </Stack>
    );
  }

  // 'en_route' with no reported next calling point is still only ever an
  // INFERENCE, never a confirmed status -- see the Alert copy below. A
  // genuinely confirmed arrival is handled entirely by the `'completed'`
  // branch above and never reaches this heuristic at all.
  const mayHaveFinished = state.status === 'en_route' && state.nextCallingPoint === null;

  return (
    <Stack gap="sm">
      <Text fw={500}>Train {state.trainUid}</Text>
      {pinSummary}
      {mayHaveFinished && (
        <Alert color="yellow" title="May have finished" variant="light">
          {/* Provisional heuristic, not a confirmed backend status -- see
              this plan's Global Constraints and
              docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
              Open Question 2. Deliberately worded as an inference
              ("may have"), never asserted as fact. */}
          No further calling points have been reported. This journey may have finished, but this is an
          inference, not a confirmed status from Network Rail.
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
          <Badge color={state.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
            {state.delayMinutes > 0 ? `${state.delayMinutes}m late` : 'On time'}
          </Badge>
        </Group>
      )}
      {state.nextCallingPoint && <Text size="sm">Next calling point: {state.nextCallingPoint}</Text>}
      <EtaBadge etaNext={state.etaNext} etaSource={state.etaSource} />
    </Stack>
  );
}
