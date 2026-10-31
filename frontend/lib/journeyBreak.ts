import { formatTime } from './dateFormat';
import { legIsOver } from './homeJourneys';
import { legDepartureInstant } from './journeyLegChaining';
import { operatorOnlyLabel } from './plannerOperators';
import type { ItineraryLegBody } from './tripPlanSave';
import type {
  BreakCause,
  JourneyLegDetail,
  OpenBreakSummary,
  Revision,
  RevisionLeg,
  RevisionLegFate,
  ReplanSuggestion,
  ReplanSuggestionLeg,
  SuggestionSet,
  TransitMode,
} from './types';
import { isUntrackedLeg, transitModeLabel } from './untrackedLeg';

/** A badge's meaning, a subset of `StateBadge`'s `StateTone` (the same
 * names, so a value passes straight through). */
export type StateTone = 'neutral' | 'good' | 'caution' | 'late' | 'early' | 'planned' | 'critical';

/* The words of mid-journey changes (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md
 * §8, §17.6): the break's title and one-line body, a suggestion's summary
 * and its delay against the booked arrival, the suggestion-to-itinerary
 * mapping `POST .../replace` takes, and the fates of an earlier plan's
 * legs. Pure, so the banner, the sheet and the home card can't drift. */

/** What a break's title is made from. Any of it may be unknown. */
export interface BreakTitleFacts {
  cause: BreakCause | null;
  /** The trigger train's departure, an instant. */
  departure: string | null;
  /** Where the trigger train runs to ("Edinburgh"). */
  trainDestinationName: string | null;
  /** Where the traveller's own leg ends, the fallback for the above. */
  legDestinationName: string | null;
  /** The change or skipped stop. */
  stationName: string | null;
}

/** "09:52 to Edinburgh", "09:52 train", "Your train", from what is known. */
function trainPhrase(time: string | null, destination: string | null, capital: boolean): string {
  if (time && destination) return `${time} to ${destination}`;
  if (time) return `${time} train`;
  if (destination) return `${capital ? 'Your' : 'your'} train to ${destination}`;
  return capital ? 'Your train' : 'your train';
}

/** The break's headline, by cause:
 *
 * - `cancelled`: "09:52 to Edinburgh cancelled"
 * - `connection_missed`: "Connection at York missed"
 * - `user_missed`: "You missed the 09:52 to Edinburgh"
 * - `part_cancelled` / `skipped_stop`: "09:52 to Edinburgh no longer stops at Durham"
 *
 * Each falls back to fewer facts rather than inventing one. Without a
 * cause (a free choice of another train): "Other ways to Edinburgh". */
export function breakTitle(facts: BreakTitleFacts): string {
  const time = facts.departure ? formatTime(facts.departure) : null;
  const destination = facts.trainDestinationName ?? facts.legDestinationName;
  switch (facts.cause) {
    case 'cancelled':
      return `${trainPhrase(time, destination, true)} cancelled`;
    case 'connection_missed':
      return facts.stationName ? `Connection at ${facts.stationName} missed` : 'Connection missed';
    case 'user_missed':
      if (time && destination) return `You missed the ${time} to ${destination}`;
      if (time) return `You missed the ${time} train`;
      return 'You missed your train';
    case 'part_cancelled':
    case 'skipped_stop': {
      const train = trainPhrase(time, destination, true);
      return facts.stationName
        ? `${train} no longer stops at ${facts.stationName}`
        : `${train} no longer stops where you need it`;
    }
    default:
      return facts.legDestinationName ? `Other ways to ${facts.legDestinationName}` : 'Other trains';
  }
}

/** The title facts of an open break on a journey page, read from the
 * break and its trigger leg. */
export function openBreakTitleFacts(openBreak: OpenBreakSummary, legs: readonly JourneyLegDetail[]): BreakTitleFacts {
  const leg = legs.find((candidate) => candidate.id === openBreak.triggerLegId) ?? null;
  return {
    cause: openBreak.cause,
    departure: leg ? legDepartureInstant(leg) : null,
    trainDestinationName: leg?.trackedTrainState?.scheduleDestinationName ?? null,
    legDestinationName: leg?.destinationName ?? null,
    stationName: openBreak.stationName,
  };
}

/** The title facts of a suggestion set (the sheet's own headline), with
 * the cause it was asked for and the break's station when there is one. */
export function suggestionSetTitleFacts(
  set: SuggestionSet,
  cause: BreakCause | null,
  stationName: string | null,
): BreakTitleFacts {
  return {
    cause,
    departure: set.trigger?.departure ?? null,
    trainDestinationName: set.trigger?.trainDestinationName ?? null,
    legDestinationName: cause === null ? set.destinationName : (set.trigger?.destinationName ?? set.destinationName),
    stationName: stationName ?? (cause === 'connection_missed' ? (set.trigger?.originName ?? set.originName) : null),
  };
}

/** The line under a break's title: "3 other ways to Edinburgh. First
 * arrives 11:12.", "No more trains to Edinburgh today." with none, or
 * "Looking for other trains." while none are planned yet. */
export function breakBody(
  count: number,
  destinationName: string | null,
  firstArrival: string | null,
  ready = true,
): string {
  if (!ready) return 'Looking for other trains.';
  const to = destinationName ? ` to ${destinationName}` : '';
  if (count === 0) return `No more trains${to} today.`;
  const ways = `${count} other ${count === 1 ? 'way' : 'ways'}${to}.`;
  return firstArrival ? `${ways} First arrives ${formatTime(firstArrival)}.` : ways;
}

/** `breakBody` for a suggestion set. */
export function suggestionSetBody(set: SuggestionSet): string {
  const arrivals = set.suggestions
    .map((suggestion) => suggestion.arrival)
    .sort((a, b) => Date.parse(a) - Date.parse(b));
  return breakBody(set.suggestions.length, set.destinationName, arrivals[0] ?? null);
}

function legOriginName(leg: ReplanSuggestionLeg): string | null {
  return leg.kind === 'train' ? (leg.originName ?? leg.originCrs) : leg.originName;
}

function legDestinationName(leg: ReplanSuggestionLeg): string | null {
  return leg.kind === 'train' ? (leg.destinationName ?? leg.destinationCrs) : leg.destinationName;
}

/** "direct", "1 change", "2 changes". */
export function changesLabel(changeCount: number): string {
  if (changeCount <= 0) return 'direct';
  return `${changeCount} ${changeCount === 1 ? 'change' : 'changes'}`;
}

/** A suggestion in one line: "10:04 York to Edinburgh, direct", "10:15
 * York to Edinburgh, 1 change". The ends are the first leg's origin and
 * the last leg's destination, else the set's own. */
export function suggestionSummary(
  suggestion: ReplanSuggestion,
  set?: Pick<SuggestionSet, 'originName' | 'destinationName'>,
): string {
  const first = suggestion.legs[0];
  const last = suggestion.legs.at(-1);
  const origin = (first ? legOriginName(first) : null) ?? set?.originName ?? null;
  const destination = (last ? legDestinationName(last) : null) ?? set?.destinationName ?? null;
  const route =
    origin && destination ? `${origin} to ${destination}` : destination ? `to ${destination}` : (origin ?? 'train');
  return `${formatTime(suggestion.departure)} ${route}, ${changesLabel(suggestion.changeCount)}`;
}

/** The legs of a suggestion in words, one per leg: "10:04 York to
 * Newcastle (LNER)", "Walk, Euston to King's Cross". */
export function suggestionLegLines(suggestion: ReplanSuggestion): string[] {
  return suggestion.legs.map((leg) => {
    const origin = legOriginName(leg);
    const destination = legDestinationName(leg);
    const route = origin && destination ? `${origin} to ${destination}` : (origin ?? destination ?? '');
    if (leg.kind === 'train') {
      const operator = leg.operatorName ? ` (${leg.operatorName})` : '';
      const cancelled = leg.cancelled ? ', cancelled' : '';
      return `${formatTime(leg.departure)} ${route}${operator}${cancelled}`.trim();
    }
    const mode = transitModeLabel(asTransitMode(leg.transitMode));
    const what = leg.lineName ? `${mode}: ${leg.lineName}` : mode;
    return route ? `${what}, ${route}` : what;
  });
}

/** A suggestion's arrival against the booked one. */
export interface SuggestionDelay {
  /** On screen: "+12 min", "On time", "−3 min". */
  text: string;
  /** For a screen reader: "Arrives 12 min after your booked time". */
  spoken: string;
  tone: StateTone;
}

/** The delay against the booked arrival (spec §8.3): "+12 min" orange,
 * red from 60 minutes, "On time" green, earlier teal. `null` when not
 * known. */
export function suggestionDelay(delayMinutes: number | null): SuggestionDelay | null {
  if (delayMinutes === null) return null;
  if (delayMinutes > 0) {
    return {
      text: `+${delayMinutes} min`,
      spoken: `Arrives ${delayMinutes} min after your booked time`,
      tone: delayMinutes >= 60 ? 'critical' : 'late',
    };
  }
  if (delayMinutes < 0) {
    return {
      text: `−${-delayMinutes} min`,
      spoken: `Arrives ${-delayMinutes} min before your booked time`,
      tone: 'early',
    };
  }
  return { text: 'On time', spoken: 'Arrives at your booked time', tone: 'good' };
}

/** One badge on a suggestion card. */
export interface SuggestionBadge {
  label: string;
  tone: StateTone;
  /** The fit's explanation, shown on tap. */
  reason: string | null;
}

/** At most this many badges in a row (docs/style-guide.md, Typography). */
const MAX_BADGES = 3;

/** A suggestion's operator filter badges (2026-10-31): "Uses other
 * operators" (yellow) when the journey's filter left no route, and "LNER
 * only" for its "prefer" filter's own route, named from its legs. */
function operatorBadges(suggestion: ReplanSuggestion): SuggestionBadge[] {
  const badges: SuggestionBadge[] = [];
  if (suggestion.outsideOperatorFilter) {
    badges.push({
      label: 'Uses other operators',
      tone: 'caution',
      reason: "No route kept to this journey's operator filter, so this one uses other operators.",
    });
  }
  if (suggestion.operatorOnly?.length) {
    const nameOf = (code: string): string => {
      for (const leg of suggestion.legs) {
        if (leg.kind === 'train' && leg.operator === code && leg.operatorName) return leg.operatorName;
      }
      return code;
    };
    badges.push({ label: operatorOnlyLabel(suggestion.operatorOnly, nameOf), tone: 'neutral', reason: null });
  }
  return badges;
}

/** A card's badges: its pick labels (gray), its operator filter badges,
 * then its ticket fit ("Check your ticket" and "May need a new ticket"
 * yellow, the rest gray), at most three, the fit and operator badges kept
 * when there are too many picks. */
export function suggestionBadges(suggestion: ReplanSuggestion): SuggestionBadge[] {
  const operators = operatorBadges(suggestion);
  const fit: SuggestionBadge | null = suggestion.fitLabel
    ? {
        label: suggestion.fitLabel,
        tone: suggestion.fit === 'check' || suggestion.fit === 'likelyNewTicket' ? 'caution' : 'neutral',
        reason: suggestion.fitReason,
      }
    : null;
  const picks = suggestion.pickLabels
    .slice(0, Math.max(0, MAX_BADGES - operators.length - (fit ? 1 : 0)))
    .map((label): SuggestionBadge => ({ label, tone: 'neutral', reason: null }));
  return fit ? [...picks, ...operators, fit] : [...picks, ...operators];
}

const TRANSIT_MODES: ReadonlySet<string> = new Set<TransitMode>([
  'tube',
  'dlr',
  'bus',
  'tram',
  'metro',
  'rail',
  'walk',
  'transfer',
  'ferry',
  'taxi',
  'cycle',
  'other',
]);

/** A wire mode as a journey mode: lower case, "other" when unknown. */
function asTransitMode(mode: string): TransitMode {
  const lower = mode.toLowerCase();
  return TRANSIT_MODES.has(lower) ? (lower as TransitMode) : 'other';
}

/** A suggestion's leg as a `POST /Journeys/itinerary` leg, which `POST
 * .../legs/{id}/replace` takes too, leaving out what is unknown. */
export function suggestionLegBody(leg: ReplanSuggestionLeg): ItineraryLegBody {
  if (leg.kind === 'train') {
    return {
      trainUid: leg.trainUid,
      serviceDate: leg.serviceDate,
      ...(leg.originCrs ? { originCrs: leg.originCrs } : {}),
      ...(leg.destinationCrs ? { destinationCrs: leg.destinationCrs } : {}),
    };
  }
  return {
    kind: 'untracked',
    transitMode: asTransitMode(leg.transitMode),
    ...(leg.originName ? { originName: leg.originName } : {}),
    ...(leg.destinationName ? { destinationName: leg.destinationName } : {}),
    ...(leg.lineName ? { lineName: leg.lineName } : {}),
    ...(leg.originStopId ? { originStopId: leg.originStopId } : {}),
    ...(leg.destinationStopId ? { destinationStopId: leg.destinationStopId } : {}),
    plannedDeparture: leg.plannedDeparture,
    plannedArrival: leg.plannedArrival,
  };
}

/** A fate's badge: what became of an earlier plan's leg. `null` for a leg
 * still planned, which needs no badge. */
export function fateBadge(fate: RevisionLegFate): { label: string; tone: StateTone } | null {
  switch (fate) {
    case 'cancelled':
      return { label: 'Cancelled', tone: 'critical' };
    case 'part_cancelled':
      return { label: 'Part cancelled', tone: 'critical' };
    case 'missed':
      return { label: 'Missed', tone: 'late' };
    case 'not_used':
      return { label: 'Not used', tone: 'neutral' };
    case 'travelled':
      return { label: 'Travelled', tone: 'neutral' };
    default:
      return null;
  }
}

/** The badge on the booked plan (blue: an arrangement, not a fault). */
export const BOOKED_BADGE = { label: 'Booked', tone: 'planned' } as const satisfies {
  label: string;
  tone: StateTone;
};

/** A plan's heading: "Booked plan", "Changed plan", "Updated plan" (moved
 * on its own) or "Earlier plan restored". */
export function revisionTitle(revision: Pick<Revision, 'reason'>): string {
  switch (revision.reason) {
    case 'booked':
      return 'Booked plan';
    case 'replace':
      return 'Changed plan';
    case 'auto_shift':
      return 'Updated plan';
    case 'revert':
      return 'Earlier plan restored';
  }
}

/** An earlier plan's leg in one line: "09:52 York to Edinburgh", or an
 * untracked leg's mode and places ("Walk, Euston to King's Cross"). */
export function revisionLegLine(leg: RevisionLeg): string {
  if (leg.kind === 'untracked') {
    const mode = transitModeLabel(asTransitMode(leg.transitMode ?? 'other'));
    const what = leg.lineName ? `${mode}: ${leg.lineName}` : mode;
    const origin = leg.originPlace ?? leg.originName;
    const destination = leg.destinationPlace ?? leg.destinationName;
    const places = origin && destination ? `${origin} to ${destination}` : (origin ?? destination);
    const time = leg.plannedDeparture ? `${formatTime(leg.plannedDeparture)} ` : '';
    return places ? `${time}${what}, ${places}` : `${time}${what}`;
  }
  const origin = leg.originName ?? leg.originCrs;
  const destination = leg.destinationName ?? leg.destinationCrs;
  const route = origin && destination ? `${origin} to ${destination}` : (origin ?? destination ?? 'Train');
  return leg.bookedDeparture ? `${formatTime(leg.bookedDeparture)} ${route}` : route;
}

/** The one line for "I missed this train" when the train before it was too
 * late to make the change: the api's `faultNote` for such a break, said
 * before anything is recorded. */
export const LATE_INBOUND_NOTE = 'Your earlier train was late, so this counts as a missed connection.';

/** Said under every list: the booked plan stays the Delay Repay baseline. */
export const BOOKED_PLAN_SAVED_NOTE = 'Your booked plan stays saved for Delay Repay.';

/** Shown after a 409 `suggestion_stale` swaps in the latest options. */
export const STALE_SUGGESTION_MESSAGE = 'That train no longer works. Here are the latest options.';

/** How long Undo stays offered after an accept. */
export const UNDO_WINDOW_MS = 30_000;

/** The leg "I missed this train" goes on: the first train leg with a
 * train that is not yet over (`legIsOver`), `null` with none. Never a
 * travelled leg. */
export function missableLegId(legs: readonly JourneyLegDetail[], now: Date): number | null {
  const leg = legs.find(
    (candidate, index) =>
      !isUntrackedLeg(candidate) &&
      candidate.trackedTrainState !== null &&
      !legIsOver(candidate, now, index === legs.length - 1),
  );
  return leg?.id ?? null;
}
