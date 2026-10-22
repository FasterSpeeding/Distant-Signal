import { formatDate } from './dateFormat';
import { routeLabel, stationLabel } from './stationLabel';
import type { GroupJourney, JourneyListItem } from './types';

/** The one rule for naming a journey, wherever it is listed, headed or
 * previewed (docs/style-guide.md, "Journey names"):
 *
 * - its own `customName`, verbatim, when the owner set one;
 * - otherwise "Origin to Destination, 17 Sep 2026": the FIRST leg's origin
 *   to the LAST leg's destination (never the current leg's, never the
 *   first leg's destination), then the first leg's date;
 * - a multi-leg journey says how many legs it has ("3 legs") on its
 *   secondary line, never in the name itself;
 * - a journey with no legs yet is "Journey with no legs yet".
 *
 * Only a date, no clock time: until a leg is matched to a train its time
 * is a search window, not a departure, and a name must not change shape
 * when a leg resolves.
 *
 * Leg-kind-agnostic: every leg has an origin, a destination and a date, a
 * rail leg or not. A leg end with a name but no station code (an untracked
 * Tube or bus leg, say) is named by its name alone. Cancelled and
 * unresolved legs name the journey like any other; the status chip, not
 * the name, says what is wrong.
 *
 * A journey with no origin to name is never "Unknown station" (DS-144): a
 * train leg saved before its train had a timetable is a normal state, not
 * a missing station. It is named by its first train ("Train W12345, 17 Sep
 * 2026", "Train W12345 to York (YRK), 17 Sep 2026") when that is known,
 * else "Journey on 17 Sep 2026", as `trackedTrainRouteLabel` names a
 * tracked train (DS-111). */

/** One end of the journey: a station code, the name resolved for it, or
 * both. */
export interface JourneyEnd {
  crs: string | null | undefined;
  name: string | null | undefined;
}

/** What a journey name is built from. Every surface adapts its own payload
 * to this (see `journeyNameInputFromLegs` for a leg list). */
export interface JourneyNameInput {
  customName: string | null | undefined;
  /** The first leg's origin. */
  origin: JourneyEnd;
  /** The last leg's destination. */
  destination: JourneyEnd;
  /** The first leg's service date ("YYYY-MM-DD"), `null` with no legs. */
  serviceDate: string | null | undefined;
  legCount: number;
  /** The first leg's train, which names a journey with no origin to name
   * ("Train W12345"). Absent where the payload has none. */
  trainUid?: string | null | undefined;
}

/** The leg fields a name reads: shared by every leg shape on the wire. */
export interface JourneyNameLeg {
  originCrs: string | null;
  originName: string | null;
  destinationCrs: string | null;
  destinationName: string | null;
  /** Absent on a template's legs: a template has no day of its own, so
   * its name is the route alone. */
  serviceDate?: string | null;
  /** The leg's train, when the surface knows it. */
  trainUid?: string | null;
}

export const NO_LEGS_JOURNEY_NAME = 'Journey with no legs yet';

/** What a journey with no origin and no train to name is called, before
 * its date: "Journey on 17 Sep 2026". */
export const UNNAMED_JOURNEY_LABEL = 'Journey';

/** Adapts an ordered leg list (journey detail, share page) to a name
 * input. Legs are assumed in `leg_order`, as every endpoint returns them. */
export function journeyNameInputFromLegs(
  customName: string | null | undefined,
  legs: readonly JourneyNameLeg[],
): JourneyNameInput {
  const first = legs[0];
  const last = legs.at(-1);
  return {
    customName,
    origin: { crs: first?.originCrs, name: first?.originName },
    destination: { crs: last?.destinationCrs, name: last?.destinationName },
    serviceDate: first?.serviceDate ?? null,
    legCount: legs.length,
    trainUid: first?.trainUid ?? null,
  };
}

/** Adapts a `GET /Journeys/mine` row. Its `originCrs`/`destinationCrs`
 * are the CURRENT leg's, which for a journey not yet under way is the first
 * leg: naming from them is the bug that showed a planned three-leg journey
 * ending at its first change. The whole-journey fields win; the current
 * leg is only the fallback for an older backend that lacks them. */
export function journeyListItemNameInput(item: JourneyListItem): JourneyNameInput {
  const whole = item.lastDestinationCrs !== undefined;
  return {
    customName: item.customName,
    origin: whole
      ? { crs: item.firstOriginCrs, name: item.firstOriginName }
      : { crs: item.originCrs, name: item.originName },
    destination: whole
      ? { crs: item.lastDestinationCrs, name: item.lastDestinationName }
      : { crs: item.destinationCrs, name: item.destinationName },
    serviceDate: item.firstServiceDate ?? item.serviceDate,
    legCount: item.legCount ?? 1,
  };
}

/** Adapts a group's shared-journey row. Same whole-journey rule; an older
 * backend's row only has the first leg's train pin to fall back to. */
export function groupJourneyNameInput(journey: GroupJourney): JourneyNameInput {
  const whole = journey.lastDestinationCrs !== undefined;
  return {
    customName: journey.customName,
    origin: whole
      ? { crs: journey.firstOriginCrs, name: journey.firstOriginName }
      : { crs: journey.pinOriginCrs, name: journey.pinOriginName },
    destination: whole
      ? { crs: journey.lastDestinationCrs, name: journey.lastDestinationName }
      : { crs: journey.pinDestinationCrs, name: journey.pinDestinationName },
    serviceDate: journey.serviceDate,
    legCount: journey.legCount,
    // The row's train is its first TRAIN leg's, which is the first leg
    // only for certain on a one-leg journey.
    trainUid: journey.legCount === 1 ? journey.trainUid : null,
  };
}

function endLabel(end: JourneyEnd): string | null {
  if (end.crs) return stationLabel(end.crs, end.name);
  return end.name?.trim() || null;
}

/** "London Kings Cross (KGX) to York (YRK)", the journey's whole route.
 * Station-coded ends go through `routeLabel`, so both ends degrade to bare
 * codes together; an end with only a name (no code) is shown by name. With
 * no origin, the first train stands in for it ("Train W12345 to York
 * (YRK)"), else "Journey"; never "Unknown station" (DS-144). */
export function journeyRouteLabel(input: JourneyNameInput): string {
  if (input.legCount === 0) return NO_LEGS_JOURNEY_NAME;
  const { origin, destination } = input;
  const from = endLabel(origin);
  const to = endLabel(destination);
  if (!from) {
    const subject = input.trainUid ? `Train ${input.trainUid}` : UNNAMED_JOURNEY_LABEL;
    return to ? `${subject} to ${to}` : subject;
  }
  if (!to) return from;
  const nameOnlyEnd = !origin.crs || !destination.crs;
  if (!nameOnlyEnd) return routeLabel(origin.crs, origin.name, destination.crs, destination.name);
  return `${from} to ${to}`;
}

/** "17 Sep 2026", the first leg's date, or `null` with no legs. */
export function journeyDateLabel(input: JourneyNameInput): string | null {
  return input.serviceDate ? formatDate(input.serviceDate) : null;
}

/** "3 legs" for a multi-leg journey, `null` for one leg or none. */
export function journeyLegCountLabel(input: JourneyNameInput): string | null {
  return input.legCount > 1 ? `${input.legCount} legs` : null;
}

/** The route and date together: "Origin to Destination, 17 Sep 2026", or
 * "Journey on 17 Sep 2026" with nothing else to name it by. */
export function journeyDefaultName(input: JourneyNameInput): string {
  const route = journeyRouteLabel(input);
  const date = journeyDateLabel(input);
  if (!date) return route;
  return route === UNNAMED_JOURNEY_LABEL ? `${route} on ${date}` : `${route}, ${date}`;
}

/** The journey's name: its custom name if set, otherwise
 * `journeyDefaultName`. */
export function journeyDisplayName(input: JourneyNameInput): string {
  return input.customName?.trim() || journeyDefaultName(input);
}

/** The secondary line under a journey's name, or `null` when there is
 * nothing to add. A custom name hides the route and date, so they come
 * back here; a multi-leg journey adds its leg count. Joined with a middle
 * dot: "London Kings Cross (KGX) to York (YRK), 17 Sep 2026 · 3 legs". */
export function journeyPreviewLine(input: JourneyNameInput): string | null {
  const parts = [input.customName?.trim() ? journeyDefaultName(input) : null, journeyLegCountLabel(input)].filter(
    (part): part is string => part !== null,
  );
  return parts.length > 0 ? parts.join(' · ') : null;
}
