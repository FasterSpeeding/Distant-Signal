import { Alert, Stack, Text, Title } from '@mantine/core';
import { redirect } from 'next/navigation';
import { getJourney, getJourneyByShareToken, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { JourneyDetailView } from '@/components/JourneyDetailView';
import { getSiteOrigin } from '@/lib/siteOrigin';

export const revalidate = 0;

/** `/journeys/shared/{token}` -- the unlisted-share-link landing page
 * (2026-09-23 unlisted-links plan, Task 6): resolves an unlisted share
 * token to a `JourneyDetail` via `getJourneyByShareToken` (Task 3, genuinely
 * unauthenticated -- no cookie required) and renders it read-only through
 * `JourneyDetailView` (Task 5). Follows the exact shape
 * `app/groups/join/[token]/page.tsx` already uses for its own token
 * resolution: an invalid/expired/revoked token gets its own explanatory
 * "Link not found" copy, never a bare `notFound()` -- the same reasoning
 * that page's own doc comment gives (`notFound()` 404s the WHOLE route,
 * which would prevent this friendly render from ever showing for a real
 * visitor).
 *
 * `journey.isOwner` on the object `getJourneyByShareToken` returns is always
 * `false` server-side (Task 2's own guarantee) -- `JourneyDetailView`
 * already hides every owner-only affordance
 * (`AddJourneyLegButton`/`ShareJourneyButton`/`ShareJourneyLinkButton`/
 * `SaveAsTemplateButton`, `JourneyLegCard`'s "Change train"/candidate
 * picker) on that flag alone, so this page adds no second, hand-written
 * gate on top of it -- its only job is resolving the token, deciding
 * redirect-vs-render, and rendering. */
/** A real share token is `crate::auth::generate_session_token()`'s own
 * shape -- 32 random bytes, base64url (`URL_SAFE_NO_PAD`) encoded -- the
 * same generator every other opaque token in this app uses (session ids,
 * group ids, invite-link tokens). Checked BEFORE `token` ever reaches
 * `getJourneyByShareToken`/`getJourney` below, which build their target URL
 * by interpolating it unencoded (`lib/api.ts`) -- a malformed value could
 * otherwise redirect that fetch somewhere this route never intended.
 * Treated exactly like an unknown/expired/revoked token (the same friendly
 * "Link not found" copy below, not a bare `notFound()`) rather than as a
 * distinct case -- from a visitor's perspective a malformed token and one
 * that just doesn't resolve are the same fact: this link doesn't work. */
function isValidShareToken(token: string): boolean {
  return /^[A-Za-z0-9_-]+$/.test(token);
}

export default async function SharedJourneyPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;

  if (!isValidShareToken(token)) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Link not found</Title>
        <Alert color="red">This share link is invalid or has been revoked. Ask whoever shared it for a new one.</Alert>
      </Stack>
    );
  }

  let journey;
  try {
    journey = await getJourneyByShareToken(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Link not found</Title>
          <Alert color="red">This share link is invalid or has been revoked. Ask whoever shared it for a new one.</Alert>
        </Stack>
      );
    }
    throw err;
  }

  // Already-authorized probe -- exact same shape as
  // `app/groups/join/[token]/page.tsx`'s own "alreadyMember" check: try the
  // normal, cookie-forwarding fetch this viewer would use on any other route
  // into the journey. Success means they're the owner or a member of a
  // group it's shared to (`journey_readable_by`, unchanged) -- send them to
  // the real page, never a second, degraded rendering of the same data.
  try {
    await getJourney(journey.id);
    redirect(`/journeys/${journey.id}`);
  } catch (err) {
    if (!(err instanceof ApiNotFoundError) && !(err instanceof ApiUnauthorizedError)) {
      throw err;
    }
    // Falls through: this viewer is relying on the token itself, either
    // because they aren't logged in at all (`ApiUnauthorizedError`) or
    // because they're logged in but neither own this journey nor belong to
    // a group it's shared to (`ApiNotFoundError`) -- `journey_readable_by`'s
    // own two negative outcomes, indistinguishable by design (see that
    // function's doc comment), and indistinguishable here for the same
    // reason: the token is what's carrying their access regardless of which
    // case they're in.
  }

  const fetchedAt = new Date().toISOString();
  // `JourneyDetailView` requires `origin` to build `ShareJourneyLinkButton`'s
  // link -- but that button only ever renders when `journey.isOwner` is
  // true, which `getJourneyByShareToken` never returns (Task 2's own
  // guarantee, see the doc comment above), so this value is never actually
  // read on this page. Resolved anyway, same unconditional pattern
  // `app/journeys/[id]/page.tsx` uses for its own identical prop, for the
  // same reason given there: a cheap header/env read, and keeping it
  // unconditional means this can't silently start passing a stale/undefined
  // origin if a future edit reorders things.
  const origin = await getSiteOrigin();

  return (
    <Stack p="lg" gap="md">
      <Text size="xs" c="dimmed">
        You&apos;re viewing this journey via a shared link.
      </Text>
      <JourneyDetailView journey={journey} fetchedAt={fetchedAt} origin={origin} />
    </Stack>
  );
}
