import { Alert, Button, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { getGroup, getGroupJoinPreview, getSessionOrLoggedOut, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { LoginButton } from '@/components/LoginButton';
import { JoinGroupButton } from '@/components/JoinGroupButton';

export const revalidate = 0;

/** Per-page Open Graph/Twitter/`<title>` metadata for a shared invite link
 * -- so pasting one into Discord/Slack/iMessage/etc. shows a group-specific
 * preview rather than generic site metadata. Fetches the same
 * unauthenticated `getGroupJoinPreview(token)` call the page component
 * itself makes below; Next's fetch request memoization dedupes the two
 * into one network call per request (see the equivalent, more detailed
 * comment on `app/train/[uid]/[date]/page.tsx`'s own `generateMetadata`;
 * the reasoning is identical here). The unauthenticated call is
 * deliberate, same as the page component's own: link-unfurler bots never
 * carry a session cookie, so this is the only way they ever see a real
 * preview instead of a fallback.
 *
 * Falls back to the root layout's site-wide metadata (returns `{}`, which
 * Next merges over that fallback) on `ApiNotFoundError`, the same error
 * the page component's own "invite link not found" branch below handles
 * -- **not** `notFound()`: calling `notFound()` from `generateMetadata`
 * 404s the whole route, not just the metadata, which pre-empted the page
 * component's friendly "invalid or expired" render for every real visitor
 * of an expired/invalid link, not only unfurler bots (confirmed via a live
 * repro -- the page component's own branch below was unreachable dead code
 * for this path). An unfurler bot seeing generic site metadata instead of
 * a group-specific preview is a fine, minor degradation; a human visitor
 * silently losing the explanation this page exists to give them is not. */
/** A real invite-link token is `crate::auth::generate_session_token()`'s
 * own shape -- 32 random bytes, base64url (`URL_SAFE_NO_PAD`) encoded
 * (`crates/api/src/data/groups.rs`'s `create_invite_link`). Checked BEFORE
 * `token` ever reaches `getGroupJoinPreview`/`getGroup` below, both of
 * which interpolate it unencoded into their target URL (`lib/api.ts`) -- a
 * malformed value could otherwise redirect one of those fetches (one of
 * them cookie-bearing, via `getGroup`) somewhere this route never
 * intended. Treated exactly like an unknown/expired invite link (the same
 * fallback each caller below already has for `ApiNotFoundError`), not as a
 * distinct case -- a malformed token and one that just doesn't resolve
 * read as the same fact to a visitor either way. */
function isValidInviteToken(token: string): boolean {
  return /^[A-Za-z0-9_-]+$/.test(token);
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ token: string }>;
}): Promise<Metadata> {
  const { token } = await params;

  if (!isValidInviteToken(token)) {
    return {};
  }

  let preview;
  try {
    preview = await getGroupJoinPreview(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return {};
    }
    throw err;
  }

  const title = `Join ${preview.groupName} — Distant Signal`;
  const description = `${preview.memberCount} member${preview.memberCount === 1 ? '' : 's'} already in ${preview.groupName}. Follow this link to join and share tracked trains with the group.`;

  return {
    title,
    description,
    openGraph: { title, description, type: 'website' },
    twitter: { card: 'summary', title, description },
  };
}

/** `/groups/join/{token}` -- confirm-before-join (spec §2.3): resolves the
 * token to a group preview (works whether or not the visitor is logged in
 * -- `getGroupJoinPreview` hits the backend's unauthenticated preview
 * route), then either shows the explicit Join button (already
 * authenticated) or a login button. `LoginButton` (review §2.16 -- this
 * used to be `LoginLink`, an underlined text link with far less visual
 * weight than the filled `JoinGroupButton` the authenticated branch below
 * renders, on the one page an invitee reaches *by definition* anonymous)
 * needs no extra plumbing to preserve this token through the OIDC redirect:
 * it captures the CURRENT page's own path via `usePathname()`
 * (`useLoginHref.ts`), and that path already IS `/groups/join/{token}` --
 * logging in and landing back here re-renders this exact page, now
 * authenticated, ready for the same explicit Join click (mirroring
 * `validate_return_to`'s existing return-to-any-same-origin-path
 * mechanism, `crates/api/src/auth.rs`).
 *
 * Review §4.6 / §3.2.3: an already-authenticated visitor who's already a
 * member -- most often the group's own owner, testing their own invite
 * link -- used to be offered "Join group" regardless, because this page
 * only ever called the unauthenticated preview above, which has no
 * concept of the CALLER's own membership. `consume_invite_link`
 * (`crates/api/src/data/groups.rs`) is idempotent (`ON CONFLICT DO
 * NOTHING`), so clicking it wouldn't have corrupted anything -- but
 * offering to "join" a group you're already in reads as broken, not
 * idempotent. `getGroup(preview.groupId)` is the probe: it 404s for a
 * non-member (`get_group_detail`'s own doc comment) and succeeds for one,
 * so its outcome is exactly the membership check needed. Only run when
 * `session.authenticated` -- an anonymous visitor can't be a member of
 * anything, and the call would just throw its own `401`. */
export default async function JoinGroupPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;

  if (!isValidInviteToken(token)) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Invite link not found</Title>
        <Alert color="red">This invite link is invalid or has expired. Ask the group for a new one.</Alert>
      </Stack>
    );
  }

  let preview;
  try {
    preview = await getGroupJoinPreview(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Invite link not found</Title>
          <Alert color="red">This invite link is invalid or has expired. Ask the group for a new one.</Alert>
        </Stack>
      );
    }
    throw err;
  }

  const session = await getSessionOrLoggedOut();

  let alreadyMember = false;
  if (session.authenticated) {
    try {
      await getGroup(preview.groupId);
      alreadyMember = true;
    } catch (err) {
      // Neither is treated as "must be a member" -- a `404` is the
      // ordinary non-member case this probe exists to detect, and an
      // `ApiUnauthorizedError` here is the same narrow lapsed-session race
      // `DeleteTrainButton`'s own doc comment names: `getSession()` above
      // and this call aren't atomic. Either way, falling through to the
      // normal `JoinGroupButton` branch is safe -- it re-checks auth on
      // its own click and every other unexpected error still propagates.
      if (!(err instanceof ApiNotFoundError) && !(err instanceof ApiUnauthorizedError)) {
        throw err;
      }
    }
  }

  // The already-member branch gets its own heading/body rather than
  // reusing "Join {name}?" with an "Open group" action bolted underneath
  // it -- that combination read as contradictory (a "Join" question
  // immediately followed by "you're already in"), not just redundant, and
  // the "Joining lets everyone..." explanation has nothing to explain to
  // someone who already has that access.
  if (alreadyMember) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>You&apos;re already in {preview.groupName}</Title>
        <Text>No need to join again — you can open the group instead.</Text>
        {/* Plain `<Link>` wrapping `Button`, not `component={Link}` on a
            Mantine polymorphic prop -- this page is a Server Component,
            and that pattern previously broke `next build`'s Server/Client
            boundary check (see `app/lines/[id]/page.tsx`'s identical
            comment on its own "Edit" link). */}
        <Link href={`/groups/${preview.groupId}`} style={{ textDecoration: 'none' }}>
          <Button>Open group</Button>
        </Link>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Join {preview.groupName}?</Title>
      <Text>
        {preview.memberCount} member{preview.memberCount === 1 ? '' : 's'} already in this group. Joining lets
        everyone in {preview.groupName} see any tracked train you choose to share into it — your other tracked
        trains and tickets stay private.
      </Text>
      {session.authenticated ? (
        <JoinGroupButton token={token} groupId={preview.groupId} />
      ) : (
        <LoginButton title="Log in — needs a Distant Signal account">
          Log in to join {preview.groupName}
        </LoginButton>
      )}
    </Stack>
  );
}
