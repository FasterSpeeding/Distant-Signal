import { Group } from '@mantine/core';
import { DelayBadge } from './DelayBadge';
import { JourneyStatusGroupBadge } from './JourneyStatusBadge';
import { ServiceModeBadge } from './ServiceModeBadge';
import { StateBadge } from './StateBadge';
import type { TrackedRowStatus } from '@/lib/trackedRowStatus';

/** The one status badge for a `/track/mine` row, whether the row is a
 * train tracked on its own or a journey (`lib/trackedRowStatus.ts` does the
 * classifying). The same status reads the same either way: the journey
 * rollup's words and colours (`JourneyStatusGroupBadge`), with the delay
 * badge standing in for "On time"/"Delayed" once the delay is known, so a
 * late train says by how much wherever it is listed.
 *
 * A finished row says how it ended: "Completed" (with its final delay),
 * "No report received" or "No train picked", all gray. A cancelled one
 * stays red, "Cancelled", or "Leg cancelled" for a multi-leg journey. */
export function TrackedRowStatusBadge({ status }: { status: TrackedRowStatus }) {
  const { kind, delayMinutes, delayProvisional } = status;
  const delay =
    delayMinutes === null ? null : <DelayBadge delayMinutes={delayMinutes} provisional={delayProvisional} />;
  switch (kind) {
    case 'timetableOnly':
      return (
        <Group gap={6} wrap="nowrap">
          <ServiceModeBadge mode={status.serviceMode} />
          <StateBadge tone="neutral">Timetabled only</StateBadge>
        </Group>
      );
    case 'completed':
      return (
        <Group gap={6} wrap="nowrap">
          <StateBadge tone="neutral">Completed</StateBadge>
          {delay}
        </Group>
      );
    case 'noReport':
      return <StateBadge tone="neutral">No report received</StateBadge>;
    case 'noTrainPicked':
      return <StateBadge tone="neutral">No train picked</StateBadge>;
    case 'legCancelled':
      return <StateBadge tone="critical">Leg cancelled</StateBadge>;
    case 'good':
    case 'delayed':
      return delay ?? <JourneyStatusGroupBadge group={kind} />;
    default:
      return <JourneyStatusGroupBadge group={kind} />;
  }
}
