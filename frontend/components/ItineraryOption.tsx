import { Badge, Card, Group, Radio, Stack, Text } from '@mantine/core';
import { codeRouteLabel } from '@/lib/stationLabel';
import type { TripPlanItinerary, TripPlanLeg } from '@/lib/types';

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
    return `${leg.scheduledDeparture.slice(0, 5)} ${route} ${leg.scheduledArrival.slice(0, 5)}`;
  }
  return `Walk/transfer (${leg.mode}) ${route}, ${leg.minutes} min`;
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
}: {
  itinerary: TripPlanItinerary;
  selected: boolean;
  onSelect: () => void;
  /** CRS -> full name, resolved by `PlanTripFlow` -- see `legSummary`'s own
   * doc comment. */
  stationNames?: Map<string, string>;
}) {
  const hasTrainLeg = itinerary.legs.some(leg => leg.kind === 'train');

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
                <Text key={index} size="sm">
                  {legSummary(leg, stationNames)}
                </Text>
              ))}
            </Stack>
          }
        />
        <Stack gap={4} align="flex-end">
          <Text size="sm">{itinerary.totalDurationMinutes} min</Text>
          <Badge color={itinerary.changeCount === 0 ? 'green' : 'blue'}>
            {itinerary.changeCount} {itinerary.changeCount === 1 ? 'change' : 'changes'}
          </Badge>
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
