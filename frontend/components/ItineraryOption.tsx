import type { ReactNode } from 'react';
import { Group, Radio, Stack, Text, VisuallyHidden } from '@mantine/core';
import { ClickableCard, selectableCardRadioProps, StaticCard } from './ClickableCard';
import { codeRouteLabel, isTiplocCode } from '@/lib/stationLabel';
import { labelPlaceEnds, RAIL_PLACE, stopPlaceKind, type PlaceKind } from '@/lib/placeKinds';
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
import {
  accessChangeClause,
  accessLegNote,
  accessStopsNote,
  changesBadge,
  transferLegLabel,
  withStationWalks,
} from '@/lib/displayLabels';
import { transitServiceModeLabel } from '@/lib/serviceMode';
import { SCHEDULED_CAPTION } from '@/lib/transitStops';
import { StateBadge } from './StateBadge';
import { StatusBadge } from './StatusBadge';
import { TransitLineSwatch } from './TransitLineSwatch';
import { LateDepartureNote, plannerLateDepartureTimes } from './LateDepartureNote';

type TransferLeg = Extract<TripPlanLeg, { kind: 'transfer' }>;

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
  if (leg.kind === 'train') {
    const name = (code: string | null) => (code ? stationNames.get(code) : undefined);
    const route = codeRouteLabel(leg.originCrs, name(leg.originCrs), leg.destinationCrs, name(leg.destinationCrs));
    // Public (timetable) times first; the working times only for a response
    // without them.
    const departure = (leg.publicDeparture ?? leg.scheduledDeparture).slice(0, 5);
    const arrival = (leg.publicArrival ?? leg.scheduledArrival).slice(0, 5);
    return `${departure} ${route} ${arrival}${liveNote(leg.live)}`;
  }
  if (leg.transitWalk) return transitWalkSummary(leg, leg.transitWalk, stationNames);
  // A TfL stop From or To (DS-135) is no station: its access leg has no
  // code at that end, only the stop, named `Uxbridge (Tube)`.
  const end = (code: string | null, stop: TflStop | undefined): PlanEnd => {
    if (code) return stationEnd(code, stationNames);
    if (stop) return { code: stop.id, name: stop.name, kind: stopPlaceKind(stop) };
    return { code: null, name: undefined };
  };
  const route = planRouteLabel(end(leg.originCrs, leg.originStop), end(leg.destinationCrs, leg.destinationStop));
  // A TfL access ride that changes trains says so in the route itself, so
  // it never reads (or is announced, inside the card's radio label) as one
  // train: "Tube (Piccadilly line) Uxbridge to Heathrow, change at Acton
  // Town, 45 min".
  const change = leg.access ? accessChangeClause(leg.access) : undefined;
  return `${transferLegLabel(leg)} ${route}${change ? `, ${change}` : ''}, ${leg.minutes} min`;
}

/** One end of a plan row: its code (a CRS, or a stop's id), its bare name
 * and what kind of station it is. */
interface PlanEnd {
  code: string | null;
  name: string | undefined;
  kind?: PlaceKind | undefined;
}

/** A transfer's station end: a National Rail station for a CRS, of no
 * known kind for a bus stop's or ferry terminal's `tiploc:` code. */
function stationEnd(code: string | null, stationNames: Map<string, string>): PlanEnd {
  if (!code) return { code: null, name: undefined };
  return { code, name: stationNames.get(code), kind: isTiplocCode(code) ? undefined : RAIL_PLACE };
}

/** A transfer row's route, "VXH · Vauxhall to EUS · Euston". Each stop end
 * says its mode (`Uxbridge (Tube)`), and a National Rail end says it is
 * one when the names would otherwise read alike (DS-239): "Vauxhall (Tube)
 * to VXH · Vauxhall (National Rail)". */
function planRouteLabel(origin: PlanEnd, destination: PlanEnd): string {
  const [originName, destinationName] = labelPlaceEnds(
    { name: origin.name ?? '', kind: origin.kind },
    { name: destination.name ?? '', kind: destination.kind },
    { alwaysTagStops: true },
  );
  return codeRouteLabel(
    origin.code,
    origin.name === undefined ? undefined : originName,
    destination.code,
    destination.name === undefined ? undefined : destinationName,
  );
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
  if (leg.serviceMode === 'replacementBus' || !leg.line) return `${departure} ${mode} ${route} ${arrival}`;
  return (
    <>
      {departure} {mode} (
      <TransitLineSwatch line={leg.line} size={10} inline />
      {leg.line.name}) {route} {arrival}
    </>
  );
}

/** A walk between a National Rail station and a tram or TfL stop (§8.1):
 * the stop end has no station code, so the row names the stop. A TfL
 * station says its mode, as a TfL stop does ("Walk VXH · Vauxhall
 * (National Rail) to Vauxhall (Tube), 4 min", DS-239); another network's
 * says it is a tram stop ("Walk MAN · Manchester Piccadilly to Piccadilly
 * tram stop, 6 min"). */
function transitWalkSummary(leg: TransferLeg, walk: TripPlanTransitWalk, stationNames: Map<string, string>): string {
  const station = stationEnd(walk.towardsStop ? leg.originCrs : leg.destinationCrs, stationNames);
  const tfl = walk.network === 'tfl';
  const stop: PlanEnd = {
    code: walk.stop.id,
    name: tfl ? walk.stop.name : `${walk.stop.name} tram stop`,
    // A TfL station from an api without its modes is "(TfL)", as before.
    kind: tfl ? (stopPlaceKind(walk.stop) ?? { kind: 'stop', modes: ['TfL'] }) : undefined,
  };
  const route = walk.towardsStop ? planRouteLabel(station, stop) : planRouteLabel(stop, station);
  return `Walk ${route}, ${leg.minutes} min`;
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
  operatorOnlyLabel,
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
  /** Words an `operatorOnly` label ("LNER only", 2026-10-31). */
  operatorOnlyLabel?: ((operators: string[]) => string) | undefined;
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
                  {/* A Tube hop's walks to and from its Tube stations are
                      rows of their own around the ride. */}
                  {(leg.kind === 'transfer' ? withStationWalks(leg) : [leg]).map((part, partIndex) => (
                    <Group key={partIndex} gap="xs" wrap="wrap">
                      <Text size="sm">{legSummary(part, stationNames)}</Text>
                      {part.kind === 'train' && <ServiceModeBadge mode={part.serviceMode} />}
                      {part.kind === 'transit' && <ServiceModeBadge transitMode={part.serviceMode} />}
                    </Group>
                  ))}
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
                <StateBadge
                  key={labelIndex}
                  tone={label.kind === 'alternative' || label.kind === 'operatorOnly' ? 'neutral' : 'action'}
                >
                  {optionLabelText(label, (code) => stationNames.get(code) ?? code, operatorOnlyLabel)}
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
