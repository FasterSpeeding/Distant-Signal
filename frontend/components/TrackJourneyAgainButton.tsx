'use client';

import { useRouter } from 'next/navigation';
import { Button } from '@mantine/core';
import { trackAgainHref } from '@/lib/trackAgainPrefill';
import type { JourneyDetail } from '@/lib/types';

/** "Track this journey again" (design doc §2.1/§6 item 1/§8 Phase A) --
 * pre-fills `/track` from this journey's own already-fetched FIRST leg
 * (see `lib/trackAgainPrefill.ts`'s own doc comment for why only the
 * first leg, and this plan's Judgment Call 2). Pure client-side
 * navigation, no fetch, no mutation of the journey being viewed at all --
 * unlike `AddJourneyLegButton`/`ShareJourneyButton`, both of which DO
 * mutate it and are correctly owner-gated by their caller
 * (`app/journeys/[id]/page.tsx`), this component takes no `isOwner` prop
 * and is meant to be shown to every viewer -- see this plan's Judgment
 * Call 7 for why that's the right call here specifically.
 *
 * Renders nothing at all when `trackAgainHref` returns `null` -- the rare
 * case of a journey whose first leg has no origin to reproduce at all
 * (`trackAgainPrefill.ts`'s own doc comment) -- same "never a dead-end
 * control" posture `ShareJourneyButton.tsx` already takes for zero
 * groups, rather than rendering a button that would open `/track` with
 * nothing usefully filled in. */
export function TrackJourneyAgainButton({ journey }: { journey: JourneyDetail }) {
  const router = useRouter();
  const href = trackAgainHref(journey);
  if (href === null) return null;

  return (
    <Button variant="default" size="xs" onClick={() => router.push(href)}>
      Track this journey again
    </Button>
  );
}
