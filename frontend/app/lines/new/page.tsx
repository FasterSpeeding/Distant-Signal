import { Center, Stack, Text, Title } from '@mantine/core';
import { CustomLineForm } from '../CustomLineForm';

// No `export const revalidate = 0` -- unlike `/lines/page.tsx` (which
// fetches four things server-side and needs it to avoid `next build`
// trying and failing to prerender against the `api` service, which only
// exists on the compose network at runtime -- see that page's own
// comment), this page fetches nothing server-side. Matches
// `app/track/page.tsx`'s existing shape: a static route with no dynamic
// segment and no server-side data fetch needs nothing here.
export default function NewCustomLinePage() {
  return (
    // `Center` plus a `maw` matching CustomLineForm's own `maw={480}`
    // keeps this chrome's width in lockstep with the form's, so the
    // heading lines up with the form's edges -- same reasoning as
    // `[id]/edit/page.tsx`'s own comment, which this page copies almost
    // verbatim (see that file for the precedent).
    <Center>
      <Stack p="lg" gap="md" maw={480} w="100%">
        <Title order={1}>New custom line</Title>
        {/* Review §2.16: `CustomLineForm`'s own "Create line" button is
            shown to every visitor, logged in or not (the Tier-2 "show the
            control, gate on the real 401" pattern `useNeedsLogin.ts`
            documents), which is right for discoverability but gives no
            hint up front that saving needs an account -- same complaint the
            review makes of the anonymous pin star (§3.4). Unconditional
            rather than gated on `getSession()`: it stays true for a logged-
            in visitor too, and checking session here would turn this
            otherwise-static route dynamic for no real gain (see this page's
            own comment above on why it fetches nothing server-side). */}
        <Text size="sm" c="dimmed">
          Creating a line needs a Distant Signal account — you&apos;ll be sent to log in when you save if you
          aren&apos;t already signed in.
        </Text>
        <CustomLineForm cancelHref="/lines" />
      </Stack>
    </Center>
  );
}
