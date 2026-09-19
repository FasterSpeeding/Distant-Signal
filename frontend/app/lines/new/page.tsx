import { Center, Stack, Title } from '@mantine/core';
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
        {/* Review §2.16/Task 3.4.13's own account-needed hint, and Task
            3.4.3's "what even is a custom line" explanation, both now live
            inside `CustomLineForm` itself (rendered unconditionally there
            for the explanation, and gated on `!existingLine` for the hint)
            rather than duplicated here as a page-level `Text` -- this page
            and `[id]/edit/page.tsx` both mount the form immediately below
            their own `<h1>` with nothing in between, so centralizing the
            copy in the one file both share avoids the two drifting apart. */}
        <CustomLineForm cancelHref="/lines" />
      </Stack>
    </Center>
  );
}
