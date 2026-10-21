import { StateBadge } from './StateBadge';
import { TrackedRowStatusBadge } from './TrackedRowStatusBadge';
import { journeyDetailRowStatus } from '@/lib/trackedRowStatus';
import { isOpenTrainLeg } from '@/lib/untrackedLeg';
import type { JourneyLegDetail } from '@/lib/types';

/** Its home was here; `TrackedRowStatusBadge.tsx` holds the label and
 * colour tables now, and this re-export keeps every import working. */
export { JourneyStatusGroupBadge } from './TrackedRowStatusBadge';

/** The journey page's header badge: the whole journey's summary
 * (`lib/journeyStatus.ts`'s `summariseJourney`, the rule the Track list
 * and the home page use too), rendered by `TrackedRowStatusBadge`. On time
 * or late is the final arrival's; a cancelled leg flags the journey;
 * a missed or tight connection shows. Renders nothing for a journey with
 * no legs (should not occur in practice).
 *
 * Review §2.5/M17: this used to wrap the `Badge` in a `Tooltip` whose
 * `label` was the exact same string the badge already renders visibly --
 * announcing nothing a sighted user didn't already have, while also being
 * unreachable by keyboard. Dropped rather than reworded.
 *
 * Finding I15/2.3: when a leg still needs a train, the badge counts the
 * unmatched legs and becomes a same-page anchor to the FIRST one, landing
 * on `id="leg-{legId}"` (`app/journeys/[id]/page.tsx` sets that id on
 * every leg's own wrapper). A plain `<a href="#...">`: the browser's
 * native same-page anchor behaviour is exactly what's wanted here. */
export function JourneyStatusBadge({ legs }: { legs: JourneyLegDetail[] }) {
  const status = journeyDetailRowStatus(legs);
  if (status === null) return null;

  if (status.kind === 'unmatched') {
    // An untracked leg (Tube, bus, walk) never has a train to pick.
    const unmatchedLegs = legs.filter(isOpenTrainLeg);
    const firstUnmatchedLegId = unmatchedLegs[0]?.id;
    const label = unmatchedLegs.length === 1 ? '1 leg needs a train' : `${unmatchedLegs.length} legs need a train`;
    if (firstUnmatchedLegId === undefined) {
      return <StateBadge tone="action">{label}</StateBadge>;
    }
    return (
      <StateBadge href={`#leg-${firstUnmatchedLegId}`} tone="action">
        {label}
      </StateBadge>
    );
  }

  return <TrackedRowStatusBadge status={status} />;
}
