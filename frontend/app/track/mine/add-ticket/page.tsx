import { Stack, Title } from '@mantine/core';
import { getSession } from '@/lib/api';
import { AutoOpenLoginPrompt } from '../AutoOpenLoginPrompt';
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
 * Proactive `getSession()` gate, same defensive `.catch()` fallback
 * `TicketPanel.tsx` already uses for an identical purpose: `/track/mine`'s
 * own entry-point Group (including the link to this page) only ever
 * renders for a visitor `getMyTrackedTrains()` has already confirmed is
 * logged in, so this page keeps that promise rather than only discovering
 * "actually, you're not logged in" reactively at submit time -- e.g. a
 * session that expired between loading /track/mine and clicking through.
 * `AutoOpenLoginPrompt` is reused as-is from the sibling /track/mine route
 * (relative import) rather than duplicated -- it already takes arbitrary
 * `children` and has no dependency on which page renders it. */
export default async function AddTicketPage() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Add a ticket</Title>
        <AutoOpenLoginPrompt>Log in to add a ticket.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Add a ticket</Title>
      <TextLink href="/track/mine">Back to My Trains &amp; Tickets</TextLink>
      {/* defaultOpen: this page's entire reason for existing is already
          stated by the Title above, so there's no reason to make a
          visitor click a button that repeats it. */}
      <TicketEntryForm label="Add a ticket" defaultOpen />
    </Stack>
  );
}
