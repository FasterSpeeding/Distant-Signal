import { Alert, Stack, Text, Title } from '@mantine/core';
import { getGroupJoinPreview, getSession, ApiNotFoundError } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { JoinGroupButton } from '@/components/JoinGroupButton';

export const revalidate = 0;

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
