import type { JourneyDetail } from './types';

/** What a caller wanting to chain another leg onto a journey needs to know
 * about its CURRENT last leg -- shared by `app/journeys/[id]/page.tsx` (the
 * "Add a leg" button on an already-created journey) and
 * `JourneyCreationFlow.tsx` (the same button, offered inline during initial
 * creation -- see that component's own doc comment for the design). Factored
 * out of the page rather than left as private, duplicated logic in two
 * places, which would otherwise be free to drift apart on exactly this "can
 * the user chain another leg on right now" judgment call.
 *
 * The station a next leg's own origin should default to -- the CURRENT
 * last leg's own `destinationCrs`, falling back to its matched train's
 * `scheduleDestinationCrs` for the case a `knownTrain`/`window`-mode leg's
 * own `destination_crs` was never set at creation time (only the bound
 * train's own schedule knows it). `null` for a journey with no legs at all,
 * or when neither source has a destination yet. */
export function journeyPriorDestinationCrs(journey: JourneyDetail): string | null {
  const lastLeg = journey.legs.at(-1) ?? null;
  return lastLeg?.destinationCrs ?? lastLeg?.trackedTrainState?.scheduleDestinationCrs ?? null;
}

/** Whether a next leg can be chained onto this journey right now. There is
 * nothing to chain a new leg onto until the CURRENT last leg has a real
 * matched train (`trackedTrainState !== null`) -- an unmatched leg has no
 * settled destination/arrival time yet for a following leg's origin to key
 * off, and `POST /Journeys/{id}/legs` has no way to validate a chain onto a
 * leg that might still resolve to nothing. `false` for a journey with no
 * legs at all (shouldn't occur in practice, but this mirrors the same
 * defensive posture `defaultJourneyTitle`/`trackAgainPrefill` already take
 * for the identical edge case rather than assuming the invariant holds). */
export function journeyCanAddLeg(journey: JourneyDetail): boolean {
  const lastLeg = journey.legs.at(-1) ?? null;
  return lastLeg !== null && lastLeg.trackedTrainState !== null;
}
