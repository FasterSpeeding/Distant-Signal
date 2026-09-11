import { Alert, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { getGroupJoinPreview, getSession, ApiNotFoundError } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
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
 * Same `notFound()`-on-`ApiNotFoundError` handling as the page component's
 * own "invite link not found" branch below -- `generateMetadata` runs
 * independently of the page component, so it needs its own equivalent
 * try/catch rather than relying on the page's. Unlike the page component
 * (which renders an in-page "invalid or expired" message so a human
 * visitor gets a helpful explanation), this just 404s: there's no metadata
 * worth showing for a token that doesn't resolve, and a plain 404 is
 * exactly what an unfurler bot should see for one. */
export async function generateMetadata({
  params,
}: {
  params: Promise<{ token: string }>;
}): Promise<Metadata> {
  const { token } = await params;

  let preview;
  try {
    preview = await getGroupJoinPreview(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
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
 * authenticated) or a login link. `LoginLink` needs no extra plumbing to
 * preserve this token through the OIDC redirect: it captures the CURRENT
 * page's own path via `usePathname()` (`useLoginHref.ts`), and that path
 * already IS `/groups/join/{token}` -- logging in and landing back here
 * re-renders this exact page, now authenticated, ready for the same
 * explicit Join click (mirroring `validate_return_to`'s existing
 * return-to-any-same-origin-path mechanism, `crates/api/src/auth.rs`). */
export default async function JoinGroupPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;

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

  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));

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
        <LoginLink underline="always">Log in to join {preview.groupName}</LoginLink>
      )}
    </Stack>
  );
}
