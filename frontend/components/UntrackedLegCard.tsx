import { Card, Group, Stack, Text } from '@mantine/core';
import { EditUntrackedLegButton } from './EditUntrackedLegButton';
import { RemoveJourneyLegButton } from './RemoveJourneyLegButton';
import { StateBadge } from './StateBadge';
import { formatDate } from '@/lib/dateFormat';
import { UNTRACKED_STATUS_LABEL, untrackedLegPlaces, untrackedLegTimes, untrackedLegTitle } from '@/lib/untrackedLeg';
import type { JourneyLegDetail } from '@/lib/types';

/** One untracked leg's card on a journey page: a Tube ride, a bus, a walk,
 * anything we have no live or timetable data for (spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2.3). Its mode and line, its places, its planned times and its state
 * from the clock alone ("Planned", "In progress (not live tracked)",
 * "Done", computed by the api). No stop list, no progress line and no
 * "Change train": there is nothing live to show, and it says so. The
 * owner can remove it like any other leg. */
export function UntrackedLegCard({
  journeyId,
  leg,
  isOwner,
  isOnlyLeg,
}: {
  journeyId: number;
  leg: JourneyLegDetail;
  isOwner: boolean;
  isOnlyLeg: boolean;
}) {
  const places = untrackedLegPlaces(leg);
  const times = untrackedLegTimes(leg);
  const status = leg.untrackedStatus ?? 'planned';
  return (
    <Card withBorder data-untracked-leg>
      <Stack gap="xs">
        <Group justify="space-between" align="flex-start" wrap="wrap" gap="xs">
          <Stack gap={2}>
            <Text fw={600} size="lg">
              {untrackedLegTitle(leg)}
            </Text>
            {places && <Text size="sm">{places}</Text>}
          </Stack>
          <Group gap="xs">
            <StateBadge tone="neutral">{UNTRACKED_STATUS_LABEL[status]}</StateBadge>
            {isOwner && <EditUntrackedLegButton journeyId={journeyId} leg={leg} />}
            {isOwner && <RemoveJourneyLegButton journeyId={journeyId} legId={leg.id} isOnlyLeg={isOnlyLeg} />}
          </Group>
        </Group>
        <Text size="sm">
          {formatDate(leg.plannedDeparture ?? `${leg.serviceDate}T12:00:00Z`)}
          {times ? `, ${times}` : ''}
        </Text>
        <Text size="xs" c="dimmed">
          Times are planned. We have no live data for this leg.
        </Text>
      </Stack>
    </Card>
  );
}
