import { Badge, Group, Stack, Text } from '@mantine/core';
import type { TrackedTrainTicket, TicketListItem, TicketSource } from '@/lib/types';
import { formatDateTime } from '@/lib/dateFormat';
import { stationLabel } from '@/lib/stationLabel';

/** Provenance labels for `TicketSummary`'s badge -- styled after
 * `IssueList.tsx`'s `DATA_QUALITY_LABELS` (`components/IssueList.tsx:38-44`),
 * this feature's own conceptual sibling per
 * `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:17-23`'s
 * own comment ("extending DESIGN.md's dataQuality philosophy"). Exact
 * wording is a naming detail, not load-bearing -- see
 * docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md's
 * Open Question 2. */
const SOURCE_LABELS: Record<TicketSource, string> = {
  manual: 'Manual entry',
  'pkpass-semantics': 'From Wallet pass',
  'pkpass-heuristic': 'From Wallet pass',
  'pdf-heuristic': 'From PDF',
};

/** The "operator — ticket type" / "origin → destination" row renderer,
 * shared by `TicketPanel.tsx` (one tracked train's own tickets) and the
 * merged `app/track/mine/page.tsx` (both a train's attached tickets and
 * its own standalone-tickets section) -- extracted out of `TicketPanel.tsx`,
 * where it was previously a private, unexported function, so both can
 * reuse it rather than duplicating ticket-row rendering. `Pick<...>` keeps
 * the prop narrow: this component only ever reads these six fields, from
 * either wire shape. `source`/`createdAt` are never `null`/`undefined` on
 * either wire shape (both are `NOT NULL` columns on `tracked_train_tickets`,
 * independent of `tracked_train_id`'s attachment status), so this
 * component needs no fallback rendering for either -- unlike
 * `operator`/`ticketType`/the CRS fields, which stay optional. */
export function TicketSummary({
  ticket,
}: {
  ticket: Pick<
    TrackedTrainTicket | TicketListItem,
    | 'operator'
    | 'ticketType'
    | 'originCrs'
    | 'destinationCrs'
    | 'originName'
    | 'destinationName'
    | 'source'
    | 'createdAt'
  >;
}) {
  // Preserves the existing '?' fallback for whichever single end is
  // missing on a ticket that has at least one CRS -- `routeLabel` assumes
  // a non-null origin, which doesn't fit this component's looser shape.
  const route =
    ticket.originCrs || ticket.destinationCrs
      ? `${ticket.originCrs ? stationLabel(ticket.originCrs, ticket.originName) : '?'} → ${
          ticket.destinationCrs ? stationLabel(ticket.destinationCrs, ticket.destinationName) : '?'
        }`
      : null;
  return (
    <Stack gap={2}>
      <Text fw={500}>
        {ticket.operator ?? 'Ticket'}
        {ticket.ticketType ? ` — ${ticket.ticketType}` : ''}
      </Text>
      {route && <Text size="sm">{route}</Text>}
      <Group gap="xs">
        {/* Explicit gray, same rationale as IssueList.tsx's dataQuality
            badge (components/IssueList.tsx:366-372): without a `color`,
            Mantine falls back to theme.primaryColor, making this read as
            branded or interactive. It's provenance, not brand. */}
        <Badge variant="outline" size="sm" color="gray">
          {SOURCE_LABELS[ticket.source]}
        </Badge>
        <Text size="xs" c="dimmed">
          Added {formatDateTime(ticket.createdAt)}
        </Text>
      </Group>
    </Stack>
  );
}
