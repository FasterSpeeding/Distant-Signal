import { Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { JourneyDetailView } from '@/components/JourneyDetailView';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { getSiteOrigin } from '@/lib/siteOrigin';

export const revalidate = 0;

/** `/journeys/[id]` -- design doc §4. One card per leg. No editable
 * header, no skip badge, no platform column -- all explicitly deferred,
 * see this plan's own Non-goals for the reasoning behind each. A
 * share-to-group button DOES exist (Task 8), and so does an unlisted
 * share-LINK button (`ShareJourneyLinkButton`, docs/superpowers/sdd/
 * 2026-09-23-unlisted-links-plan Task 4) -- but both only for the
 * journey's owner (`journey.isOwner`) -- a non-owning group member
 * reaches this page via `journey_readable_by`'s group-shared read path
 * and gets neither button nor the leg-level owner-only controls
 * (`JourneyLegCard`'s own `isOwner` gating). The actual rendering (header,
 * status badge, owner-only actions, leg cards, connectors) lives in
 * `components/JourneyDetailView.tsx` (2026-09-23 unlisted-links plan, Task
 * 5) -- extracted so a later `/journeys/shared/[token]` page can reuse it
 * for an anonymous share-link viewer; only the "Back to my trains &
 * journeys" link below stays specific to this page, since that anonymous
 * viewer has no "my trains" to go back to. */
export default async function JourneyDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  if (!/^\d+$/.test(id)) {
    notFound();
  }

  let journey;
  try {
    journey = await getJourney(Number(id));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    // Same distinct "log in, this might be yours" posture
    // app/train/by-id/[trackingId]/page.tsx already takes for the
    // identical 401-vs-404 split, for the same reason: this page has no
    // public sibling content to fall back to.
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Someone&apos;s tracked journey — log in to see it</Title>
          <LoginLink underline="always">Log in to view this journey</LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  // `revalidate = 0` above means this page is a live, uncached fetch on
  // every request -- so "the instant this request's own `getJourney()`
  // call returned" genuinely IS when every fact on this page (the delay
  // figure, the last-reported location, every leg's status) was current
  // as of, not an invented number. 2026-09-22 UX review finding I24/2.12:
  // nothing on this page previously said when "22m late" was measured,
  // and a journey page is MORE time-critical than a single train page --
  // a stale leg-1 ETA silently invalidates a leg-2 pick.
  const fetchedAt = new Date().toISOString();
  // Only actually used by `ShareJourneyLinkButton` (owner-only, rendered
  // inside `JourneyDetailView` below), but resolved unconditionally rather
  // than behind an `if (journey.isOwner)` -- same reasoning
  // `app/groups/[id]/page.tsx` gives for its own identical unconditional
  // `getSiteOrigin()` call: it's a cheap header/env read, and keeping it
  // unconditional means this can't silently start passing a stale/
  // undefined origin if a future edit reorders things.
  const origin = await getSiteOrigin();

  return (
    <Stack p="lg" gap="md">
      {/* The way back out. This page is reached by a `router.push` from
          the create flow and had no breadcrumb at all, so closing the tab
          lost the journey unless the user had memorised `/journeys/169`
          (2026-09-22 UX review, C1). `/track/mine` now lists journeys, so
          this link has a real destination. */}
      <TextLink href="/track/mine" underline="always">
        Back to my trains &amp; journeys
      </TextLink>
      <JourneyDetailView journey={journey} fetchedAt={fetchedAt} origin={origin} />
    </Stack>
  );
}
