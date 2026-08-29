import { Alert, Badge, Group, Loader, Stack, Text } from '@mantine/core';
import { EtaBadge } from './EtaBadge';
import { formatDate } from '@/lib/dateFormat';
import type { TrackedTrainState } from '@/lib/types';

/** Renders one `TrackedTrainState` through every state the backend can
 * return, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 3's table. Shared by both `/train/by-id/[trackingId]` and
 * `/train/[uid]/[date]`.
 *
 * Note on the pin summary shown for `pending`/`unresolved`: this renders
 * origin, destination, and service *date* only -- `TrackedTrainState` has
 * no scheduled-departure clock-time field (see this component's own
 * module in the implementation plan for why), so this does not claim to
 * show a scheduled time the backend doesn't return. */
export function TrainJourney({ state }: { state: TrackedTrainState }) {
  const pinSummary = (
    <Text size="sm" c="dimmed">
      {state.pinOriginCrs}
      {state.pinDestinationCrs ? ` → ${state.pinDestinationCrs}` : ''} · {formatDate(state.serviceDate)}
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
          This train hasn&apos;t been matched to a live service yet. This page updates automatically.
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
        <JourneyDetails state={state} />
      </Stack>
    );
  }

  // 'en_route' or 'completed' share the same "current position" rendering
  // -- 'completed' is kept as a real branch even though no current
  // trust-consumer code path produces it yet (see this plan's Global
  // Constraints and Status note), so it's forward-compatible rather than
  // dead code the day journey.rs gets real completion detection.
  const mayHaveFinished =
    state.status === 'completed' || (state.status === 'en_route' && state.nextCallingPoint === null);

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
      <JourneyDetails state={state} />
    </Stack>
  );
}

function JourneyDetails({ state }: { state: TrackedTrainState }) {
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
