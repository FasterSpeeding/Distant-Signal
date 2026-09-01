import { Divider, Group, Stack, Text, Title } from '@mantine/core';
import { getMyTickets } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import { TicketSummary } from '@/components/TicketSummary';
import { TicketEntryForm } from '@/components/TicketEntryForm';
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
 * `getMyTickets()` returning `null` on a `401` is the COMPLETE "not logged
 * in" signal for this page -- there is no second party to disambiguate
 * (no id in this route's path that could belong to someone else), so no
 * separate `getSession()` call is needed here the way `TicketPanel` needs
 * one.
 *
 * Superseded by the merged `/track/mine` page (Part B of the upload-first
 * plan -- see that page's own doc comment) but kept working here rather
 * than half-migrated: this file still renders standalone tickets
 * (`trackedTrainId: null`, per Part A) correctly, just without an attach
 * action of their own (that lives on the merged page, which has the
 * caller's tracked-train list already loaded to attach against). Now DOES
 * have its own "Add a ticket" entry point (`TicketEntryForm` with no
 * `trackingId`) -- Part A's real, working entry point for the upload-first
 * flow, added here ahead of the full merge. */
export default async function MyTicketsPage() {
  const tickets = await getMyTickets();

  if (tickets === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Tickets</Title>
        <TextLink href="/api/auth/login" underline="always">
          Log in to see your tickets
        </TextLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>My Tickets</Title>
      {tickets.length === 0 ? (
        <Text c="dimmed" component="div">
          You haven&apos;t added any tickets yet. Add one below, or track a train first and attach a ticket to it
          from that train&apos;s own page. <TextLink href="/track" underline="always">Track a train</TextLink> to
          get started.
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
      <TicketEntryForm label="Add a ticket" />
    </Stack>
  );
}

function TicketListRow({ ticket }: { ticket: TicketListItem }) {
  // A standalone ticket (Part A -- `trackedTrainId: null`) has no owning
  // tracked train to link to at all yet, unlike the `resolved`-with-null-
  // `trainUid` case below (defensive: the backend's own resolution
  // invariant means that shouldn't happen, but this component doesn't
  // assume it). Canonical, shareable URL once resolved; the by-id detail
  // route otherwise -- same logic as the sibling tracked-trains list's own
  // row link, applied to a ticket's owning tracked train.
  const href =
    ticket.trackedTrainId === null
      ? null
      : ticket.resolutionStatus === 'resolved' && ticket.trainUid && ticket.serviceDate
        ? `/train/${ticket.trainUid}/${ticket.serviceDate}`
        : `/train/by-id/${ticket.trackedTrainId}`;

  return (
    <Stack gap="xs">
      <Group justify="space-between" wrap="nowrap">
        <TicketSummary ticket={ticket} />
        {href ? (
          <TextLink href={href}>
            {ticket.serviceDate && ticket.pinScheduledDeparture
              ? `${formatDate(ticket.serviceDate)} · ${formatTime(ticket.pinScheduledDeparture)}`
              : 'View train'}
          </TextLink>
        ) : (
          <Text size="sm" c="dimmed">
            Not yet attached to a tracked train
          </Text>
        )}
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
