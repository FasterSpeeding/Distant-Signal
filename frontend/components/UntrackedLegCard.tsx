import { Card, Group, Stack, Text } from '@mantine/core';
import { EditUntrackedLegButton } from './EditUntrackedLegButton';
import { RemoveJourneyLegButton } from './RemoveJourneyLegButton';
import { StateBadge } from './StateBadge';
import { TextLink } from './TextLink';
import { TimetabledLegDetails } from './TimetabledLegDetails';
import { formatDate } from '@/lib/dateFormat';
import {
  UNTRACKED_STATUS_LABEL,
  untrackedLegPlaceNames,
  untrackedLegPlaces,
  untrackedLegTimes,
  untrackedLegTitle,
  type UntrackedLegNeighbours,
} from '@/lib/untrackedLeg';
import { stopPageHref } from '@/lib/transitStops';
import type { JourneyLegDetail } from '@/lib/types';

/** One untracked leg's card on a journey page: a Tube ride, a bus, a walk,
 * anything we have no live data for (spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2.3). Its mode and line, its places, its planned times and its state
 * from the clock alone ("Planned", "In progress (not live tracked)",
 * "Done", computed by the api). No stop list, no progress line and no
 * "Change train": there is nothing live to show, and it says so. The
 * owner can remove it like any other leg. A timetabled leg (DS-237), a
 * Tube, DLR or tram ride saved from a plan, also shows what its network's
 * timetable says about it (`TimetabledLegDetails`), still scheduled only. */
export function UntrackedLegCard({
  journeyId,
  leg,
  isOwner,
  isOnlyLeg,
  neighbours,
}: {
  journeyId: number;
  leg: JourneyLegDetail;
  isOwner: boolean;
  isOnlyLeg: boolean;
  /** The legs either side, which can say what kind of station each end
   * is when its names read alike (DS-239). */
  neighbours?: UntrackedLegNeighbours | undefined;
}) {
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
            <UntrackedLegPlaces leg={leg} neighbours={neighbours} />
          </Stack>
          <Group gap="xs">
            {leg.timetable && <StateBadge tone="neutral">Scheduled</StateBadge>}
            <StateBadge tone="neutral">{UNTRACKED_STATUS_LABEL[status]}</StateBadge>
            {isOwner && <EditUntrackedLegButton journeyId={journeyId} leg={leg} />}
            {isOwner && <RemoveJourneyLegButton journeyId={journeyId} legId={leg.id} isOnlyLeg={isOnlyLeg} />}
          </Group>
        </Group>
        <Text size="sm">
          {formatDate(leg.plannedDeparture ?? `${leg.serviceDate}T12:00:00Z`)}
          {times ? `, ${times}` : ''}
        </Text>
        {leg.timetable ? (
          <TimetabledLegDetails leg={leg} timetable={leg.timetable} />
        ) : (
          <Text size="xs" c="dimmed">
            Times are planned. We have no live data for this leg.
          </Text>
        )}
      </Stack>
    </Card>
  );
}

/** "Euston to Victoria", with each place picked from a stop linking to its
 * page: a Metrolink station's (`9400ZZMA...`) on the transit pages, a TfL
 * stop's (`940G...`, London trams included) on `/tfl/stops`. Plain text
 * for free-text places. */
function UntrackedLegPlaces({
  leg,
  neighbours,
}: {
  leg: JourneyLegDetail;
  neighbours?: UntrackedLegNeighbours | undefined;
}) {
  const places = untrackedLegPlaces(leg, neighbours);
  if (!places) return null;
  const originHref = stopPageHref(leg.originStopId);
  const destinationHref = stopPageHref(leg.destinationStopId);
  if (!originHref && !destinationHref) return <Text size="sm">{places}</Text>;
  const place = (name: string, href: string | null) =>
    href ? (
      <TextLink href={href} underline="always" inline size="sm">
        {name}
      </TextLink>
    ) : (
      name
    );
  const names = untrackedLegPlaceNames(leg, neighbours);
  const origin = names.originName ? place(names.originName, originHref) : null;
  const destination = names.destinationName ? place(names.destinationName, destinationHref) : null;
  return (
    <Text size="sm">
      {origin && destination ? (
        <>
          {origin} to {destination}
        </>
      ) : origin ? (
        <>From {origin}</>
      ) : (
        <>To {destination}</>
      )}
    </Text>
  );
}
