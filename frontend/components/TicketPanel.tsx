import { Divider, Stack, Text } from '@mantine/core';
import { getSession, getTicketsForTrackedTrain, getDelayRepayEstimate } from '@/lib/api';
import { LoginLink } from './LoginLink';
import { TicketEntryForm } from './TicketEntryForm';
import { DelayRepayEstimate } from './DelayRepayEstimate';
import type { TrackedTrainTicket } from '@/lib/types';

/** Renders on both `/train/by-id/[trackingId]` and `/train/[uid]/[date]`,
 * directly below `<TrainJourney>`. This is a real, session-gated feature
 * with NO unauthenticated read path at all (Decision 4 of
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md)
 * layered onto two PUBLIC pages (any viewer, owner or not, can load either
 * page, per the train-tracking-frontend spec) -- so this component's own
 * ownership probe *is* the entire "is this yours" check for the whole
 * ticket feature (Decision 1).
 *
 * Branches on four real, distinguishable outcomes. `401` and `404` from
 * `GET .../tickets` both collapse to `null` from `getTicketsForTrackedTrain`
 * (see that function's own doc comment in `lib/api.ts`), so this component
 * separately calls the already-established `getSession()` first to tell
 * "not logged in at all" apart from "logged in, but not the owner of this
 * pin" -- the two cases Decision 1 requires rendering completely
 * differently (a login nudge vs. nothing at all). This composition, not a
 * change to `getTicketsForTrackedTrain`'s own spec-pinned signature, is how
 * this plan resolves that gap -- see this plan's own top-level note on it. */
export async function TicketPanel({ trackingId }: { trackingId: number }) {
  // Same defensive fallback as app/layout.tsx: an auth-status glitch should
  // degrade to the login nudge, not break this whole page for every
  // visitor. This component has no route-level `error.tsx` boundary of its
  // own (`app/error.tsx` is the root boundary), so an uncaught rejection
  // here would otherwise take down the entire tracked-train page.
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  if (!session.authenticated) {
    // Worded as "attach a ticket," not "see your ticket" -- logging in
    // doesn't guarantee this viewer owns this particular pin, so the copy
    // doesn't promise something a subsequent not-the-owner outcome might
    // immediately take back.
    //
    // No outer <Text> wrapper: TextLink already renders its own Mantine
    // <Text> (a <p> by default), so wrapping it in another <Text> would
    // nest a <p> inside a <p> -- invalid HTML and a React dev warning.
    // Rendered directly, matching the established local convention for
    // this exact "inline TextLink to /api/auth/login" login nudge (see
    // PinToggle.tsx and TrackTrainForm.tsx).
    return (
      <LoginLink underline="always">
        Log in to attach a ticket to this journey
      </LoginLink>
    );
  }

  const tickets = await getTicketsForTrackedTrain(trackingId);
  if (tickets === null) {
    // Logged in, but not the owner of this pin (or, redundantly, a
    // tracking id that already 404'd the page itself upstream). Every
    // tracked-train page is public and shareable by design, so this is
    // the overwhelming common case for a page view -- render nothing, not
    // a permanent "this isn't your journey" banner (Decision 1's own
    // reasoning for why this branch stays silent rather than explicit).
    return null;
  }

  if (tickets.length === 0) {
    return <TicketEntryForm trackingId={trackingId} label="Add a ticket for this journey" />;
  }

  // Eager, one fetch per ticket, not a client-triggered "check
  // eligibility" button -- consistent with this app's existing "just
  // refetch, no manual poll control" posture (Decision 5; flagged in the
  // design spec's Open Question 1 as fine for the expected common case of
  // a handful of tickets per tracked train, not resolved further here).
  const withEstimates = await Promise.all(
    tickets.map(async (ticket) => ({
      ticket,
      estimate: await getDelayRepayEstimate(trackingId, ticket.id),
    })),
  );

  return (
    <Stack gap="lg">
      {withEstimates.map(({ ticket, estimate }, index) => (
        <Stack key={ticket.id} gap="xs">
          {index > 0 && <Divider />}
          <TicketSummary ticket={ticket} />
          {estimate && <DelayRepayEstimate response={estimate} />}
        </Stack>
      ))}
      <TicketEntryForm trackingId={trackingId} label="Add another ticket" />
    </Stack>
  );
}

function TicketSummary({ ticket }: { ticket: TrackedTrainTicket }) {
  const route =
    ticket.originCrs || ticket.destinationCrs ? `${ticket.originCrs ?? '?'} → ${ticket.destinationCrs ?? '?'}` : null;
  return (
    <Stack gap={2}>
      <Text fw={500}>
        {ticket.operator ?? 'Ticket'}
        {ticket.ticketType ? ` — ${ticket.ticketType}` : ''}
      </Text>
      {route && <Text size="sm">{route}</Text>}
    </Stack>
  );
}
