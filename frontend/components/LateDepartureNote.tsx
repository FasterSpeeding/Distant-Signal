import { Text } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { LateDepartureReliance, TripPlanLateDepartureReliance } from '@/lib/types';

/** Whose departure the note is about: the leg it sits on ("this train"),
 * on the connector between two legs the one after it ("the next train"),
 * or in a list of legs the train named by where it goes
 * (`trainToSubject`). */
export type LateDepartureSubject = 'this train' | 'the next train' | `the train to ${string}`;

/** "the train to Edinburgh", else "the next train" with no name. */
export function trainToSubject(destinationName: string | null | undefined): LateDepartureSubject {
  return destinationName ? `the train to ${destinationName}` : 'the next train';
}

/** The note's words: "Relies on this train running late. It's due to
 * leave at 10:35, booked 10:20. It could leave earlier." `scheduled` and
 * `expected` are "HH:MM" clock times. */
export function lateDepartureText(scheduled: string, expected: string, subject: LateDepartureSubject): string {
  return `Relies on ${subject} running late. It's due to leave at ${expected}, booked ${scheduled}. It could leave earlier.`;
}

/** A `/Trips/plan` leg's `live.reliesOnLateDeparture` ("HH:MM:SS" clock
 * times) as the note's two times. */
export function plannerLateDepartureTimes(reliance: TripPlanLateDepartureReliance): {
  scheduled: string;
  expected: string;
} {
  return { scheduled: reliance.scheduledDeparture.slice(0, 5), expected: reliance.expectedDeparture.slice(0, 5) };
}

/** A journey connection's or an alternative's `reliesOnLateDeparture`
 * (instants) as the note's two UK clock times. */
export function instantLateDepartureTimes(reliance: LateDepartureReliance): { scheduled: string; expected: string } {
  return { scheduled: formatTime(reliance.scheduledDeparture), expected: formatTime(reliance.expectedDeparture) };
}

/** The calm caution shown where a change works only because the train
 * changed onto is running late (DS-141; `reliesOnLateDeparture` in
 * docs/api-changelog.md): the planner's leg, the journey page's connector,
 * the home page's current journey and the alternatives sheet. Caution, not
 * a fault, so the light yellow text colour rather than red or orange. */
export function LateDepartureNote({
  scheduled,
  expected,
  subject = 'this train',
}: {
  scheduled: string;
  expected: string;
  subject?: LateDepartureSubject;
}) {
  return (
    <Text size="xs" c="var(--mantine-color-yellow-light-color)" data-late-departure-note>
      {lateDepartureText(scheduled, expected, subject)}
    </Text>
  );
}
