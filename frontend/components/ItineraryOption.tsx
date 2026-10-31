import type { ReactNode } from 'react';
import { Group, Radio, Stack, Text, VisuallyHidden } from '@mantine/core';
import { ClickableCard, selectableCardRadioProps, StaticCard } from './ClickableCard';
import { codeRouteLabel, tflStopPlaceName } from '@/lib/stationLabel';
import { ServiceModeBadge } from './ServiceModeBadge';
import {
  accessLegStopCount,
  optionLabelText,
  satisfiedStationLabel,
  viaSatisfiedLabel,
  waypointSatisfiedLabel,
} from '@/lib/tripPlan';
import { CANCELLED_LABEL, delayLabel } from '@/lib/serviceStatus';
import type { StationGroup } from '@/lib/stationGroups';
import type {
  TripPlanItinerary,
  TripPlanLeg,
  TripPlanLegLive,
  TripPlanResponse,
  TripPlanTransitLeg,
  TripPlanTransitWalk,
  TripPlanViaSatisfied,
  TripPlanWaypointSatisfied,
  TflStop,
} from '@/lib/types';
import { accessChangeClause, accessLegNote, accessStopsNote, changesBadge, transferLabel } from '@/lib/displayLabels';
import { transitServiceModeLabel } from '@/lib/serviceMode';
import { SCHEDULED_CAPTION } from '@/lib/transitStops';
import { StateBadge } from './StateBadge';
import { StatusBadge } from './StatusBadge';
import { TransitLineSwatch } from './TransitLineSwatch';
import { LateDepartureNote, plannerLateDepartureTimes } from './LateDepartureNote';

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
function legSummary(leg: TripPlanLeg, stationNames: Map<string, string>): ReactNode {
  if (leg.kind === 'transit') return transitLegSummary(leg);
  if (leg.kind === 'transfer' && leg.transitWalk) return transitWalkSummary(leg.transitWalk, leg.minutes);
  // A TfL stop From or To (DS-135) is no station: its access leg has no
  // code at that end, only the stop, named `Uxbridge (Tube)`.
  const end = (code: string | null, stop: TflStop | undefined): [string | null, string | undefined] => {
    if (code) return [code, stationNames.get(code)];
    if (stop) return [stop.id, tflStopPlaceName(stop)];
    return [null, undefined];
  };
  const [originCode, originName] = end(leg.originCrs, leg.kind === 'transfer' ? leg.originStop : undefined);
  const [destinationCode, destinationName] = end(
    leg.destinationCrs,
    leg.kind === 'transfer' ? leg.destinationStop : undefined,
  );
  const route = codeRouteLabel(originCode, originName, destinationCode, destinationName);
  if (leg.kind === 'train') {
    // Public (timetable) times first; the working times only for a response
    // without them.
    const departure = (leg.publicDeparture ?? leg.scheduledDeparture).slice(0, 5);
    const arrival = (leg.publicArrival ?? leg.scheduledArrival).slice(0, 5);
    return `${departure} ${route} ${arrival}${liveNote(leg.live)}`;
  }
  // A TfL access ride that changes trains says so in the route itself, so
  // it never reads (or is announced, inside the card's radio label) as one
  // train: "Tube (Piccadilly line) Uxbridge to Heathrow, change at Acton
  // Town, 45 min".
  const change = leg.access ? accessChangeClause(leg.access) : undefined;
  return `${transferLabel(leg.mode, leg.line)} ${route}${change ? `, ${change}` : ''}, ${leg.minutes} min`;
}

/** A tram leg (transit trip planning §11.3): "08:14 Tram (Blue Line)
 * Piccadilly to Victoria 08:22", the line's name after its colour, as the
 * network page's line cards show it. A replacement bus has no line:
 * "08:14 Replacement bus Piccadilly to Victoria 08:22". Its times are the
 * timetable's, never live. */
function transitLegSummary(leg: TripPlanTransitLeg): ReactNode {
  const departure = leg.scheduledDeparture.slice(0, 5);
  const arrival = leg.scheduledArrival.slice(0, 5);
  const mode = transitServiceModeLabel(leg.serviceMode) ?? 'Tram';
  const route = `${leg.originStop.name} to ${leg.destinationStop.name}`;
  if (leg.serviceMode !== 'tram' || !leg.line) return `${departure} ${mode} ${route} ${arrival}`;
  return (
    <>
      {departure} {mode} (
      <TransitLineSwatch line={leg.line} size={10} inline />
      {leg.line.name}) {route} {arrival}
    </>
  );
}

/** A walk between a station and a tram stop (§8.1): the tram end has no
 * station code, so the row names the stop. */
function transitWalkSummary(walk: TripPlanTransitWalk, minutes: number): string {
  return `Walk ${walk.towardsStop ? 'to' : 'from'} ${walk.stop.name} tram stop, ${minutes} min`;
}

/** The note under a tram walk: its minutes are worked out from where the
 * stop and station are, not measured. */
function transitWalkNote(walk: TripPlanTransitWalk): string {
  return `About ${walk.walkMinutes} min walk${walk.estimated ? ', estimated' : ''}`;
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
  stopAccess,
  operatorName,
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
  /** The plan's TfL stop ends and their candidates (`TripPlanResponse`'s
   * `access`), for how many stops an access ride rides. */
  stopAccess?: TripPlanResponse['access'];
  /** Names a train, bus or ferry leg's operator (`leg.operator`, an ATOC
   * code) under it (2026-10-31); without it, no operator is shown. */
  operatorName?: ((code: string) => string) | undefined;
}) {
  const hasTrainLeg = itinerary.legs.some((leg) => leg.kind === 'train');
  const changes = changesBadge(itinerary);
  const stationLabelOf = (hit: { crs: string; matchedCrs?: string | null }) =>
    satisfiedStationLabel(
      hit,
      (code) => stationNames.get(code) ?? code,
      (code) => groups.find((group) => group.code === code)?.name,
    );

  // A route with a train is a selectable card: its radio is the target and
  // a click anywhere on the card picks it. One with no train can't be
  // picked, so it is flat.
  const content = (
    <>
      <Group justify="space-between" wrap="wrap">
        <Radio
          {...(hasTrainLeg && selectableCardRadioProps)}
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
                    <Text size="sm">{legSummary(leg, stationNames)}</Text>
                    {leg.kind === 'train' && <ServiceModeBadge mode={leg.serviceMode} />}
                    {leg.kind === 'transit' && <ServiceModeBadge transitMode={leg.serviceMode} />}
                  </Group>
                  {leg.kind === 'transfer' && leg.access && (
                    <Text size="xs" c="dimmed" pl="sm">
                      {accessLegNote(leg.access, accessLegStopCount(leg, stopAccess))}
                    </Text>
                  )}
                  {leg.kind === 'transfer' && leg.access && accessStopsNote(leg.access) && (
                    <Text size="xs" c="dimmed" pl="sm">
                      {accessStopsNote(leg.access)}
                    </Text>
                  )}
                  {leg.kind === 'train' && leg.operator && operatorName && (
                    <Text size="xs" c="dimmed" pl="sm" data-leg-operator>
                      {operatorName(leg.operator)}
                    </Text>
                  )}
                  {leg.kind === 'transfer' && leg.estimatedWalk && (
                    <Text size="xs" c="dimmed" pl="sm">
                      Estimated walk, instead of the Tube
                    </Text>
                  )}
                  {leg.kind === 'transfer' && leg.transitWalk && (
                    <Text size="xs" c="dimmed" pl="sm">
                      {transitWalkNote(leg.transitWalk)}
                    </Text>
                  )}
                  {leg.kind === 'transit' && <TransitLegNotes leg={leg} />}
                  {leg.kind === 'train' && leg.live?.reliesOnLateDeparture && (
                    <LateDepartureNote {...plannerLateDepartureTimes(leg.live.reliesOnLateDeparture)} />
                  )}
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
              {itinerary.scheduledOnly && (
                <Text size="xs" c="dimmed" data-scheduled-only>
                  {SCHEDULED_CAPTION}
                </Text>
              )}
            </Stack>
          }
        />
        <Stack gap={4} align="flex-end">
          {/* "Compare options" (DS-196): what this option is. */}
          {itinerary.labels && itinerary.labels.length > 0 && (
            <Group gap={4} justify="flex-end" data-option-labels>
              {itinerary.labels.map((label, labelIndex) => (
                <StateBadge key={labelIndex} tone={label.kind === 'alternative' ? 'neutral' : 'action'}>
                  {optionLabelText(label, (code) => stationNames.get(code) ?? code)}
                </StateBadge>
              ))}
            </Group>
          )}
          <Text size="sm">{itinerary.totalDurationMinutes} min</Text>
          {/* Every change, a TfL ride's included (DS-205): "1 change", with
              "1 Tube change" in its tooltip and for screen readers. */}
          <StateBadge
            tone={changes.total === 0 ? 'good' : 'neutral'}
            {...(changes.breakdown !== undefined && { title: changes.breakdown })}
          >
            {changes.label}
            {changes.breakdown !== undefined && <VisuallyHidden> ({changes.breakdown})</VisuallyHidden>}
          </StateBadge>
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
          This route needs no train, so there is nothing to track.
        </Text>
      )}
    </>
  );
  return hasTrainLeg ? (
    <ClickableCard selected={selected}>{content}</ClickableCard>
  ) : (
    <StaticCard>{content}</StaticCard>
  );
}

/** Under a tram leg: how often trams run, when there are enough to say,
 * and any disruption reported on its line, in the line status's colour.
 * Shown, never acted on: the plan still uses the timetable. */
function TransitLegNotes({ leg }: { leg: TripPlanTransitLeg }) {
  return (
    <>
      {leg.headwayMinutes !== undefined && (
        <Text size="xs" c="dimmed" pl="sm">
          About every {leg.headwayMinutes} min
        </Text>
      )}
      {leg.notices.map((notice, index) => (
        <Stack key={index} gap={2} pl="sm" pt={2} data-transit-leg-notice>
          <Group gap="xs" wrap="wrap">
            <StatusBadge severity={notice.severity} />
            <Text size="xs">{notice.message}</Text>
          </Group>
          {notice.reason && (
            <Text size="xs" c="dimmed">
              {notice.reason}
            </Text>
          )}
        </Stack>
      ))}
    </>
  );
}
