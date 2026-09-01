import { Divider, Group, Stack, Text, Title } from '@mantine/core';
import { getMyTickets } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { TicketSummary } from '@/components/TicketSummary';
import { DelayRepayEstimate } from '@/components/DelayRepayEstimate';
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { TicketListItem } from '@/lib/types';

// See app/page.tsx and this repo's other dynamic routes for the same
// `revalidate = 0` rationale: without it, Next.js treats this route as
// eligible for static generation and tries to prerender it during
// `next build`, which fails since the `api` service only exists on the
// compose network at runtime.
export const revalidate = 0;

/** `/track/tickets` -- every ticket a logged-in user has attached, across
 * every train they've tracked, most-recently-added first, per
 * docs/superpowers/specs/2026-08-31-tickets-list-design.md Decision 3.
 * Standalone rather than a section of `/track/mine`: that page does not
 * exist yet (see this plan's Status note), and per Decision 3 a
 * ticket-focused view answers a different question ("which of these is
 * worth an actual Delay Repay claim right now") than a train-focused one.
 * `getMyTickets()` returning `null` on a `401` is the COMPLETE "not logged
 * in" signal for this page -- there is no second party to disambiguate
 * (no id in this route's path that could belong to someone else), so no
 * separate `getSession()` call is needed here the way `TicketPanel` needs
 * one. This page has no "add a ticket" affordance of its own -- ticket
 * creation always needs a concrete `trackingId` in context, which this
 * cross-train list doesn't supply one specific instance of; the empty
 * state below links to `/track` (not `/track/mine`, which doesn't exist
 * yet) as the only entry point that exists today. */
export default async function MyTicketsPage() {
  const tickets = await getMyTickets();

  if (tickets === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Tickets</Title>
        <LoginLink underline="always">
          Log in to see your tickets
        </LoginLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>My Tickets</Title>
      {tickets.length === 0 ? (
        <Text c="dimmed" component="div">
          You haven&apos;t added any tickets yet. Track a train, then attach a ticket to it from that
          train&apos;s own page. <TextLink href="/track" underline="always">Track a train</TextLink> to get started.
        </Text>
      ) : (
        <Stack gap="lg">
          {tickets.map((ticket, index) => (
            <Stack key={ticket.id} gap="xs">
              {index > 0 && <Divider />}
              <TicketListRow ticket={ticket} />
            </Stack>
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function TicketListRow({ ticket }: { ticket: TicketListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- same logic as the sibling tracked-trains list's own row
  // link, applied to a ticket's owning tracked train. The
  // `resolved`-with-null-`trainUid` fallback is defensive: the backend's
  // own resolution invariant means this shouldn't happen, but this
  // component doesn't assume it.
  const href =
    ticket.resolutionStatus === 'resolved' && ticket.trainUid
      ? `/train/${ticket.trainUid}/${ticket.serviceDate}`
      : `/train/by-id/${ticket.trackedTrainId}`;

  return (
    <Stack gap="xs">
      <Group justify="space-between" wrap="nowrap">
        <TicketSummary ticket={ticket} />
        <TextLink href={href}>
          {formatDate(ticket.serviceDate)} · {formatTime(ticket.pinScheduledDeparture)}
        </TextLink>
      </Group>
      {/* Imported and used exactly as-is, no new props, no wrapper -- this
          is the literal reuse the design spec's Finding 7 identifies, and
          it's what makes the safety-critical disclaimer rendering
          automatically inherited rather than re-implemented on this page.
          This page adds no new Delay Repay rendering logic of its own. */}
      <DelayRepayEstimate
        response={{
          delayMinutes: ticket.delayMinutes,
          estimate: ticket.estimate,
          claimUrl: ticket.claimUrl,
          disclaimer: ticket.disclaimer,
        }}
      />
    </Stack>
  );
}
