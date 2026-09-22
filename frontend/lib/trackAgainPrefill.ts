import type { JourneyDetail, JourneyLegDetail } from './types';

/** What a fresh `/track` visit would need to reproduce one leg's own
 * search criteria -- the shared shape `trackAgainHref` (below) turns into
 * a query string, and the shape Phase B's "promote this journey to a
 * template" feature (docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
 * §6 item 2) is expected to reuse directly when it builds a
 * `journey_template_legs` row instead of a URL -- see this plan's own
 * closing note. Deliberately has no `serviceDate`/departure-time field at
 * all: "again" always means a NEW occurrence, so the date/time is always
 * left to whatever `/track`'s own form already defaults to (today/now),
 * never carried over from the journey being repeated. */
export interface TrackAgainPrefill {
  mode: 'pick' | 'window';
  origin: string | null;
  destination: string | null;
  /** "HH:MM", only ever populated when `mode === 'window'`. */
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
}

/** `"HH:MM:SS" | null` -> `"HH:MM" | null` -- `journey_legs.depart_after`
 * etc. are wall-clock bounds, no timezone conversion needed, just trimming
 * the seconds a passenger never entered. Same trick
 * `JourneyLegCard.tsx`'s own (private) `formatWindowTime` already applies
 * to the same four fields for display -- not imported from there since
 * it's a one-line slice and importing a display helper from a card
 * component into a data-extraction module would be the wrong direction of
 * coupling. */
function toHHMM(value: string | null): string | null {
  return value ? value.slice(0, 5) : null;
}

/** Extracts "again" criteria from a journey's FIRST leg only -- see this
 * plan's Judgment Call 2 for why only the first leg, not the whole
 * multi-leg shape. Returns `null` only for the defensive case of a
 * journey with zero legs (shouldn't happen in practice -- removing a
 * journey's only leg removes the journey itself, per
 * `RemoveJourneyLegButton.tsx` -- but `JourneyDetailPage`'s own
 * `defaultJourneyTitle` guards the identical case the same way, so this
 * matches established house style rather than assuming the invariant
 * holds).
 *
 * `mode` is derived from whether ANY of the leg's four window bounds is
 * set, not from `matchMode` -- a matched window-mode leg keeps its window
 * fields populated forever (they're never cleared on match), so this is
 * the only reliable signal for "was this originally a time-window search,
 * or a direct pin/known-train pick" -- see Judgment Call 3. `origin`
 * prefers the leg's own `originCrs`, falling back to the matched train's
 * own `pinOriginCrs` for the rare case a `knownTrain`-mode leg's own
 * `origin_crs` was `null` at creation time (the bound `trains` row had no
 * schedule data yet) -- same fallback shape `lib/journeyLegLabel.ts`'s
 * `legEndpointName` already uses for the analogous name-resolution
 * problem. `destination` follows the identical pattern. */
export function trackAgainPrefill(journey: JourneyDetail): TrackAgainPrefill | null {
  const firstLeg: JourneyLegDetail | undefined = journey.legs[0];
  if (!firstLeg) return null;

  const hasWindow =
    firstLeg.departAfter !== null ||
    firstLeg.departBefore !== null ||
    firstLeg.arriveAfter !== null ||
    firstLeg.arriveBefore !== null;

  const origin = firstLeg.originCrs ?? firstLeg.trackedTrainState?.pinOriginCrs ?? null;
  const destination =
    firstLeg.destinationCrs ?? firstLeg.trackedTrainState?.pinDestinationCrs ?? null;

  return {
    mode: hasWindow ? 'window' : 'pick',
    origin,
    destination,
    departAfter: hasWindow ? toHHMM(firstLeg.departAfter) : null,
    departBefore: hasWindow ? toHHMM(firstLeg.departBefore) : null,
    arriveAfter: hasWindow ? toHHMM(firstLeg.arriveAfter) : null,
    arriveBefore: hasWindow ? toHHMM(firstLeg.arriveBefore) : null,
  };
}

/** `trackAgainPrefill`, encoded as a `/track` query string --
 * `TrackJourneyAgainButton.tsx`'s only dependency on this module. Returns
 * `null` whenever there's no origin to reproduce at all (the rare gap
 * noted on `trackAgainPrefill`'s own `origin` field) -- never a link to a
 * form that would open with nothing usefully filled in, same "never a
 * dead-end control" posture `ShareJourneyButton.tsx` takes for zero
 * groups. `destination` and the four window bounds are each added only
 * when actually known -- an absent query param and an empty string mean
 * the same thing to `TrackPage`'s own `Array.isArray(x) ? x[0] : x`
 * unwrapping, but omitting genuinely-unknown fields keeps the resulting
 * URL honest and short rather than papering it with empty `=`s. */
export function trackAgainHref(journey: JourneyDetail): string | null {
  const prefill = trackAgainPrefill(journey);
  if (!prefill || !prefill.origin) return null;

  const params = new URLSearchParams();
  params.set('mode', prefill.mode);
  params.set('origin', prefill.origin);
  if (prefill.destination) params.set('destination', prefill.destination);
  if (prefill.mode === 'window') {
    if (prefill.departAfter) params.set('departAfter', prefill.departAfter);
    if (prefill.departBefore) params.set('departBefore', prefill.departBefore);
    if (prefill.arriveAfter) params.set('arriveAfter', prefill.arriveAfter);
    if (prefill.arriveBefore) params.set('arriveBefore', prefill.arriveBefore);
  }
  return `/track?${params.toString()}`;
}
