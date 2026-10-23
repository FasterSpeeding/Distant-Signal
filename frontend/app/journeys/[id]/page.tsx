import { Group, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { JourneyDetailView } from '@/components/JourneyDetailView';
import { LoginControl } from '@/components/LoginControl';
import { TextLink } from '@/components/TextLink';
import { FollowJourneyButton } from '@/components/FollowJourneyButton';
import { followedFromLabel } from '@/lib/followedFrom';
import { getSiteOrigin } from '@/lib/siteOrigin';
import type { Metadata } from 'next';

export const revalidate = 0;

export const metadata: Metadata = { title: 'Journey', robots: { index: false } };

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
 * for an anonymous share-link viewer; only the "Back to journeys"
 * link below stays specific to this page, since that anonymous
 * viewer has no "my trains" to go back to. */
export default async function JourneyDetailPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  /** The break push's deep link: `?break={breakId}&src=push` (mid-journey
   * changes spec 2026-10-22 §6.4). */
  searchParams?: Promise<{ break?: string | string[]; src?: string | string[] }>;
}) {
  const { id } = await params;
  const query = (await searchParams) ?? {};
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
          <Title order={1}>Log in to see this tracked journey</Title>
          <LoginControl underline="always">Log in to view this journey</LoginControl>
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
          (2026-09-22 UX review, C1). `/journeys` now lists journeys, so
          this link has a real destination. */}
      <TextLink href="/journeys" underline="always">
        Back to journeys
      </TextLink>
      {/* Following (DS-146): a journey shared with the caller can be
          followed into their own lists, read-only. Unfollowing one
          followed through a share link goes back to the list: the caller
          can't open it any more. */}
      {journey.followedFrom ? (
        <Group gap="sm" wrap="wrap" justify="space-between">
          <Text size="sm" c="dimmed">
            {followedFromLabel(journey.followedFrom)}
          </Text>
          <FollowJourneyButton
            journeyId={journey.id}
            following
            afterUnfollowHref={journey.followedFrom.via === 'link' ? '/journeys' : undefined}
          />
        </Group>
      ) : (
        journey.canFollow && <FollowJourneyButton journeyId={journey.id} following={false} />
      )}
      <JourneyDetailView
        journey={journey}
        fetchedAt={fetchedAt}
        origin={origin}
        // The deep link opens the suggestions only on the owner's own,
        // still open break; an old link just shows the page.
        openBreakOnLoad={journey.isOwner && journey.openBreak != null && query.break === String(journey.openBreak.id)}
        fromPush={query.src === 'push'}
      />
    </Stack>
  );
}
