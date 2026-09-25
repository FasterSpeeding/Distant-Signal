import { Alert, Stack, Text, Title } from '@mantine/core';
import { redirect } from 'next/navigation';
import type { Metadata } from 'next';
import { getJourney, getJourneyByShareToken, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { JourneyDetailView } from '@/components/JourneyDetailView';
import { getSiteOrigin } from '@/lib/siteOrigin';
import { formatDate } from '@/lib/dateFormat';
import { worstLegStatus, type LegStatusGroup } from '@/lib/journeyStatus';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

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

/** Coarse, non-numeric phrasing for a journey's worst-leg status, built on
 * `lib/journeyStatus.ts::worstLegStatus` -- the same "reduce every leg to
 * one worst status" rollup `JourneyStatusBadge` already renders on-page as
 * its single header badge. Deliberately coarser than
 * `app/train/[uid]/[date]/page.tsx`'s own `trainStatusSummary`, which
 * quotes an exact `delayMinutes` figure and the train's own last reported
 * LOCATION: that page is fully public regardless of whether its own OG
 * metadata exists (no token gates it), so its preview card reveals nothing
 * a URL-guesser couldn't already get. A journey share link is different --
 * its whole security model is a bearer token (2026-09-23 unlisted-links
 * design §3/§5), and a pasted link's unfurled preview card is seen by
 * EVERYONE who can see that chat message, not only the person the owner
 * actually chose to share the link with. A live delay figure or a
 * signalling-level "last reported at X" is a materially bigger, more
 * precisely time-stamped disclosure to that wider, passive audience than
 * the coarse "delayed"/"on time"/"cancelled" rollup this app already
 * treats as the public-facing summary of a journey's status elsewhere.
 * Someone who actually opens the link with the token still sees the full
 * detail (`JourneyLegCard`'s own per-leg delay/location, `JourneyStatusBadge`
 * itself) exactly as before -- this function only governs what a passer-by
 * sees without clicking anything. */
function journeyStatusPhrase(legs: JourneyLegDetail[]): string {
  const worst = worstLegStatus(legs);
  if (worst === null) return 'has no legs yet';
  const PHRASE: Record<LegStatusGroup, string> = {
    good: 'on time',
    awaiting: 'awaiting its first movement report',
    unmatched: 'still needs a train picked',
    delayed: 'running delayed',
    skipped: 'not stopping at one of its own stations',
    severe: 'cancelled',
  };
  return PHRASE[worst];
}

/** Per-page Open Graph/Twitter/`<title>` metadata for a journey's unlisted
 * share link -- so pasting one into Discord/Slack/iMessage/etc. shows a
 * journey-specific preview rather than generic site metadata. Fetches the
 * same unauthenticated `getJourneyByShareToken(token)` call the page
 * component itself makes below; Next's fetch request memoization dedupes
 * the two into one network call per request (see the more detailed
 * comment on `app/train/[uid]/[date]/page.tsx`'s own `generateMetadata`;
 * the reasoning is identical here). The unauthenticated call is
 * deliberate, same as the page component's own: link-unfurler bots never
 * carry a session cookie, so this is the only way they ever see a real
 * preview instead of a fallback.
 *
 * Falls back to `{}` (the root layout's site-wide metadata) on
 * `ApiNotFoundError` -- the same error the page component's own "link not
 * found" branch below handles -- **not** `notFound()`, for the exact
 * reason `app/groups/join/[token]/page.tsx`'s own `generateMetadata` gives
 * for the identical choice: calling `notFound()` here would 404 the WHOLE
 * route, not just the metadata, pre-empting the page component's own
 * friendly "link not found" render for every real visitor of an
 * expired/revoked link, not only unfurler bots. Same malformed-token
 * short-circuit as the page component's own `isValidShareToken` check,
 * for the same reason that page's doc comment gives.
 *
 * Deliberately never includes the token itself, or the journey's own
 * numeric `id`, anywhere in the returned metadata -- the token is the
 * bearer secret that grants access to this journey at all, and embedding
 * it in a publicly-crawlable `<title>`/`og:` tag would hand that secret to
 * every unfurler bot and search index that ever sees the pasted link,
 * independent of whether a human ever clicks it. Nor does it include the
 * journey's own `customName` (an owner-chosen free-text label, unlike a
 * group's `groupName` in `groups/join`'s own equivalent function) -- title
 * and description are built from the route and a coarse status only, the
 * same "route is the identity" shape `defaultJourneyTitle`
 * (`components/JourneyDetailView.tsx`) already falls back to for a journey
 * with no custom name, kept unconditional here rather than conditional on
 * one being set. `JourneyDetail` carries no owner name/id at all to leak
 * in the first place (`journey_readable_by`'s own read-only, ownership-
 * blind contract). */
export async function generateMetadata({
  params,
}: {
  params: Promise<{ token: string }>;
}): Promise<Metadata> {
  const { token } = await params;

  if (!isValidShareToken(token)) {
    return {};
  }

  let journey: JourneyDetail;
  try {
    journey = await getJourneyByShareToken(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return {};
    }
    throw err;
  }

  const firstLeg = journey.legs[0];
  const lastLeg = journey.legs.at(-1) ?? firstLeg;
  const origin = firstLeg?.originName ?? firstLeg?.originCrs ?? null;
  const destination = lastLeg?.destinationName ?? lastLeg?.destinationCrs ?? null;
  const title =
    origin && destination ? `${origin} to ${destination} — Distant Signal` : 'Shared journey — Distant Signal';
  const description = firstLeg
    ? `A journey on ${formatDate(firstLeg.serviceDate)}, ${journeyStatusPhrase(journey.legs)}.`
    : 'A shared journey on Distant Signal.';

  return {
    title,
    description,
    openGraph: { title, description, type: 'website' },
    twitter: { card: 'summary', title, description },
  };
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
