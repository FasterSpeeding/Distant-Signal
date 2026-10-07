import { Badge, Card, Group, Radio, Stack, Text } from '@mantine/core';
import { codeRouteLabel } from '@/lib/stationLabel';
import { RouteText } from './RouteArrow';
import { ServiceModeBadge } from './ServiceModeBadge';
import { satisfiedStationLabel, viaSatisfiedLabel, waypointSatisfiedLabel } from '@/lib/tripPlan';
import { CANCELLED_LABEL, delayLabel } from '@/lib/serviceStatus';
import type { StationGroup } from '@/lib/stationGroups';
import type {
  TripPlanItinerary,
  TripPlanLeg,
  TripPlanLegLive,
  TripPlanViaSatisfied,
  TripPlanWaypointSatisfied,
} from '@/lib/types';

/** `stationNames` resolves a leg's bare `originCrs`/`destinationCrs` to a
 * full name -- `GET /Trips/plan` (unlike every other station-bearing
 * response in this app) never sends one itself, so `PlanTripFlow` (the
 * only caller) resolves it client-side (`lib/suggestions.ts`'s
 * `getStationNames`) from every code `lib/tripPlan.ts`'s
 * `collectTripPlanStationCodes` finds across the whole plan, and passes
 * the result down here. Empty by default so a caller with no lookup
 * result yet (or a test exercising this component in isolation) still
 * renders the bare-code fallback `codeRouteLabel` already provides for an
 * unresolved code. */
function legSummary(leg: TripPlanLeg, stationNames: Map<string, string>): string {
  const originName = leg.originCrs ? stationNames.get(leg.originCrs) : undefined;
  const destinationName = leg.destinationCrs ? stationNames.get(leg.destinationCrs) : undefined;
  const route = codeRouteLabel(leg.originCrs, originName, leg.destinationCrs, destinationName);
  if (leg.kind === 'train') {
    // Public (timetable) times first; the working times only for a response
    // without them.
    const departure = (leg.publicDeparture ?? leg.scheduledDeparture).slice(0, 5);
    const arrival = (leg.publicArrival ?? leg.scheduledArrival).slice(0, 5);
    return `${departure} ${route} ${arrival}${liveNote(leg.live)}`;
  }
  return `Walk/transfer (${leg.mode}) ${route}, ${leg.minutes} min`;
}

/** A short live annotation for a train leg (`GET /Trips/plan`'s live
 * overlay), in the shared status words (`lib/serviceStatus.ts`): empty
 * when nothing is known or the train is on time. */
function liveNote(live: TripPlanLegLive | null | undefined): string {
  if (!live) return '';
  if (live.cancelled) return ` · ${CANCELLED_LABEL}`;
  if (live.delayMinutes !== null && live.delayMinutes !== 0) return ` · ${delayLabel(live.delayMinutes).text}`;
  if (live.status === 'Delayed') return ' · Delayed';
  return '';
}

/** One selectable itinerary card -- design spec §5.3's "route-summary
 * display for both `'fastest'` and `'options'` responses." Disabled with an
 * explanatory message when the itinerary has no train leg at all (this
 * plan's own Judgment Call 4 -- there is nothing a `POST /Journeys` call
 * could create for a walk with no train on either side of it). */
export function ItineraryOption({
  itinerary,
  selected,
  onSelect,
  stationNames = new Map(),
  viaPasses = [],
  waypointStops = [],
  groups = [],
}: {
  itinerary: TripPlanItinerary;
  selected: boolean;
  onSelect: () => void;
  /** CRS -> full name, resolved by `PlanTripFlow` -- see `legSummary`'s own
   * doc comment. */
  stationNames?: Map<string, string>;
  /** This itinerary's share of its journey's `viaSatisfiedBy`: the
   * "Pass through" stations its own legs passed, each shown under the leg
   * that passed it. */
  viaPasses?: TripPlanViaSatisfied[] | undefined;
  /** This itinerary's share of its journey's `waypointSatisfiedBy`
   * (2026-10-07, only when a "Call at" stop is a group): where it stopped
   * for each, shown under the legs. */
  waypointStops?: TripPlanWaypointSatisfied[] | undefined;
  /** The station groups, to name a group a via or stop satisfied. */
  groups?: StationGroup[] | undefined;
}) {
  const hasTrainLeg = itinerary.legs.some((leg) => leg.kind === 'train');
  const stationLabelOf = (hit: { crs: string; matchedCrs?: string | null }) =>
    satisfiedStationLabel(
      hit,
      (code) => stationNames.get(code) ?? code,
      (code) => groups.find((group) => group.code === code)?.name,
    );

  return (
    <Card withBorder>
      <Group justify="space-between" wrap="wrap">
        <Radio
          checked={selected}
          onChange={onSelect}
          disabled={!hasTrainLeg}
          label={
            <Stack gap={4}>
              {itinerary.legs.map((leg, index) => (
                // A bus or ferry leg keeps `kind: 'train'`; its own badge
                // says what it really is (and that it isn't tracked live).
                <Stack key={index} gap={0}>
                  <Group gap="xs" wrap="wrap">
                    <Text size="sm">
                      <RouteText>{legSummary(leg, stationNames)}</RouteText>
                    </Text>
                    {leg.kind === 'train' && <ServiceModeBadge mode={leg.serviceMode} />}
                  </Group>
                  {viaPasses
                    .filter((via) => via.leg === index)
                    .map((via, viaIndex) => (
                      <Text key={viaIndex} size="xs" c="dimmed" pl="sm">
                        {viaSatisfiedLabel(via, stationLabelOf(via))}
                      </Text>
                    ))}
                </Stack>
              ))}
              {waypointStops.map((stop, stopIndex) => (
                <Text key={`stop-${stopIndex}`} size="xs" c="dimmed">
                  {waypointSatisfiedLabel(stop, stationLabelOf(stop))}
                </Text>
              ))}
            </Stack>
          }
        />
        <Stack gap={4} align="flex-end">
          <Text size="sm">{itinerary.totalDurationMinutes} min</Text>
          <Badge color={itinerary.changeCount === 0 ? 'green' : 'gray'} tt="none">
            {itinerary.changeCount} {itinerary.changeCount === 1 ? 'change' : 'changes'}
          </Badge>
          {itinerary.liveFeasible === false && (
            <Text size="xs" c="red">
              Live data says this route may no longer work
            </Text>
          )}
          {itinerary.exceedsRecommendedChanges && (
            <Text size="xs" c="orange">
              More changes than usually recommended
            </Text>
          )}
        </Stack>
      </Group>
      {!hasTrainLeg && (
        <Text size="xs" c="dimmed" mt="xs">
          This route needs no train — there is nothing to track.
        </Text>
      )}
    </Card>
  );
}
