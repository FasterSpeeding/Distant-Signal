import { Badge, Card, Group, Radio, Stack, Text } from '@mantine/core';
import type { TripPlanItinerary, TripPlanLeg } from '@/lib/types';

function legSummary(leg: TripPlanLeg): string {
  if (leg.kind === 'train') {
    const from = leg.originCrs ?? '?';
    const to = leg.destinationCrs ?? '?';
    return `${leg.scheduledDeparture.slice(0, 5)} ${from} → ${to} ${leg.scheduledArrival.slice(0, 5)}`;
  }
  const from = leg.originCrs ?? '?';
  const to = leg.destinationCrs ?? '?';
  return `Walk/transfer (${leg.mode}) ${from} → ${to}, ${leg.minutes} min`;
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
}: {
  itinerary: TripPlanItinerary;
  selected: boolean;
  onSelect: () => void;
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
                  {legSummary(leg)}
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
