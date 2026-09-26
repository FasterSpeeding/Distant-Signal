import { Stack, Text, Title } from '@mantine/core';
import { getSessionOrLoggedOut } from '@/lib/api';
import { AutoOpenLoginPrompt } from '../AutoOpenLoginPrompt';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { TicketEntryForm } from '@/components/TicketEntryForm';

// See app/page.tsx's own `revalidate = 0` comment for the rationale: this
// route has no dynamic segment, and it fetches getSession() server-side
// below, so without this Next.js treats it as eligible for static
// generation and tries to prerender it during `next build`, which fails
// since the `api` service only exists on the compose network at runtime.
export const revalidate = 0;

/** `/track/mine/add-ticket` -- the standalone ("no tracked train yet")
 * case of `TicketEntryForm`, moved off the bottom of `/track/mine` onto
 * its own dedicated page per
 * docs/superpowers/specs/2026-09-02-standalone-ticket-entry-page-design.md.
 * `TicketPanel.tsx`'s two trackingId-scoped instances (attaching a ticket
 * to an already-tracked, specific train) are a different, narrower
 * context and are untouched by this page.
 *
 * Proactive `getSession()` gate, same defensive `getSessionOrLoggedOut()`
 * fallback `TicketPanel.tsx` already uses for an identical purpose:
 * `/track/mine`'s own entry-point Group (including the link to this page)
 * only ever renders for a visitor `getMyTrackedTrains()` has already
 * confirmed is logged in, so this page keeps that promise rather than
 * only discovering "actually, you're not logged in" reactively at submit
 * time -- e.g. a session that expired between loading /track/mine and
 * clicking through. `AutoOpenLoginPrompt` is reused as-is from the
 * sibling /track/mine route (relative import) rather than duplicated --
 * it already takes arbitrary `children` and has no dependency on which
 * page renders it. */
export default async function AddTicketPage() {
  const session = await getSessionOrLoggedOut();

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Add a ticket</Title>
        {/* Server-rendered, same pattern as
            app/train/by-id/[trackingId]/page.tsx's own
            ApiUnauthorizedError branch: a link-unfurler bot or a
            pre-hydration visitor sees this sentence even though it can
            never run the client-only AutoOpenLoginPrompt modal below,
            which stays as progressive enhancement on top of it. */}
        <LoginLink underline="always">Log in to add a ticket</LoginLink>
        <AutoOpenLoginPrompt>Log in to add a ticket.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Add a ticket</Title>
      {/* Task 3.6.5: this page never said which train the ticket attaches
          to -- because, per `TicketEntryForm.tsx`'s own doc comment, it
          doesn't attach to one yet at all (a STANDALONE ticket, no
          `trackingId`). One dimmed sentence states that up front instead
          of leaving it implicit until the post-save "find or track the
          train" next step. */}
      <Text size="sm" c="dimmed">
        Save the ticket now; you can attach it to a tracked train afterwards, or we&apos;ll try to match it for you.
      </Text>
      <TextLink href="/track/mine">Back to My Trains &amp; Tickets</TextLink>
      {/* defaultOpen: this page's entire reason for existing is already
          stated by the Title above, so there's no reason to make a
          visitor click a button that repeats it. */}
      <TicketEntryForm label="Add a ticket" defaultOpen />
    </Stack>
  );
}
