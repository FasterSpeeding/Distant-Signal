import { Badge } from '@mantine/core';
import { worstLegStatus, type LegStatusGroup } from '@/lib/journeyStatus';
import type { JourneyLegDetail } from '@/lib/types';

// 2026-09-22 UX review finding M22/2.11: "On track" here and "On time" on
// `TrainJourney.tsx`'s own per-leg delay badge (and `JourneyTimeline.tsx`'s
// per-stop one) named the identical state two different ways, one line
// apart on `journey-168-desktop.png`. "On time" is the app's own more
// established term for this state -- it's what both of those other,
// pre-existing badges already say -- so this rollup badge aligns to it
// rather than the other way round.
const LABEL: Record<LegStatusGroup, string> = {
  good: 'On time',
  awaiting: 'Awaiting first report',
  unmatched: 'Needs a train picked',
  delayed: 'Delayed',
  severe: 'Cancelled',
};

const COLOR: Record<LegStatusGroup, string> = {
  good: 'green',
  awaiting: 'gray',
  unmatched: 'blue',
  delayed: 'yellow',
  severe: 'red',
};

/** The journey-level summary badge, per spec §3. Renders nothing for a
 * journey with no legs (should not occur in practice).
 *
 * 2026-09-22 UX review finding M17: the previous version wrapped this in a
 * `Tooltip` whose label was the exact same text the badge already shows --
 * "a hover-only tooltip on a non-focusable element that adds nothing".
 * Dropped outright rather than given new content, per the review's own
 * first-choice recommendation.
 *
 * Finding I15/2.3: when the worst status is `unmatched`, this used to say
 * the flat "Needs a train picked" with no indication of WHICH leg or HOW
 * MANY -- on mobile the badge sits roughly 800px above the card it refers
 * to. It now (a) counts the unmatched legs and (b) becomes a same-page
 * anchor to the FIRST one, landing on `id="leg-{legId}"`
 * (`app/journeys/[id]/page.tsx` sets that id on every leg's own wrapper).
 * A plain `<a href="#...">` -- no client JS needed for the scroll itself,
 * the browser's native same-page anchor behaviour is exactly what's
 * wanted here. */
export function JourneyStatusBadge({ legs }: { legs: JourneyLegDetail[] }) {
  const worst = worstLegStatus(legs);
  if (worst === null) return null;

  if (worst === 'unmatched') {
    const unmatchedLegs = legs.filter((leg) => leg.trackedTrainState === null);
    const firstUnmatchedLegId = unmatchedLegs[0]?.id;
    const label = unmatchedLegs.length === 1 ? '1 leg needs a train' : `${unmatchedLegs.length} legs need a train`;
    return (
      <Badge
        component={firstUnmatchedLegId !== undefined ? 'a' : undefined}
        href={firstUnmatchedLegId !== undefined ? `#leg-${firstUnmatchedLegId}` : undefined}
        color={COLOR.unmatched}
        variant="light"
        tt="none"
        style={firstUnmatchedLegId !== undefined ? { cursor: 'pointer' } : undefined}
      >
        {label}
      </Badge>
    );
  }

  return (
    <Badge color={COLOR[worst]} variant="light" tt="none">
      {LABEL[worst]}
    </Badge>
  );
}
