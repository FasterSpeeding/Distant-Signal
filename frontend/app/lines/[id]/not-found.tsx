import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';
import { LoginLink } from '@/components/LoginLink';
import { getSessionOrLoggedOut } from '@/lib/api';

// Task 3.4.10: an async Server Component, same as any other route file --
// Next.js's special `not-found.tsx` supports this the same way `page.tsx`
// does, it just receives no route params. `getSession()`'s use of
// `next/headers` `cookies()` opts this segment into dynamic rendering the
// same way it already does for every other page here that calls it.
export default async function LineNotFound() {
  // Anonymous vs. signed-in changes what this page offers: a session that
  // lapsed while looking at an owner-only route (e.g. `/lines/[id]/edit`)
  // collapses to this same 404 -- see `[id]/page.tsx`'s own "never confirm
  // or deny a private line's existence" rationale -- so an anonymous
  // visitor specifically gets a way back to log in, without this page
  // claiming the line does or doesn't exist. A failed session check
  // degrades to showing the link: an extra "Log in" offered to someone
  // already signed in is harmless, where hiding it from someone genuinely
  // logged out strands them. `getSessionOrLoggedOut()` (`lib/api.ts`) is
  // what does the degrading -- it still logs the failure first, since a
  // failed check here is never a confirmed "not logged in".
  const session = await getSessionOrLoggedOut();
  const isAnonymous = !session.authenticated;

  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx's fuller comment on this
          same pattern -- page-level h1, rendered size unchanged. */}
      <Title order={1} size="h2">Line not found</Title>
      <Text c="dimmed">No line matches that ID.</Text>
      {/* The page previously had no link anywhere on it — a genuine dead
          end reachable from a stale bookmark or a deleted custom line. */}
      <Group gap="lg">
        <TextLink href="/lines" underline="always">
          Browse all lines
        </TextLink>
        {/* Was "Back to your dashboard" -- meaningless to an anonymous
            visitor, who has none. Neutral wording works for both. */}
        <TextLink href="/" underline="always">
          Go to the home page
        </TextLink>
        {isAnonymous && <LoginLink underline="always">Log in</LoginLink>}
      </Group>
    </Stack>
  );
}
