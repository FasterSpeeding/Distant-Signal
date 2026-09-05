import { Badge, Group, Stack, Text } from '@mantine/core';
import type { TrackedTrainTicket, TicketListItem, TicketSource } from '@/lib/types';
import { stationLabel } from '@/lib/stationLabel';
import { LocalDateTime } from './LocalDateTime';

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
    | 'customName'
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
        {ticket.customName ?? (
          <>
            {ticket.operator ?? 'Ticket'}
            {ticket.ticketType ? ` — ${ticket.ticketType}` : ''}
          </>
        )}
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
        {/* The one viewer-local timestamp in the app, and deliberately so:
            "Added" records something *this viewer* did, so their own clock
            is what answers it -- the same convention an email or
            notification timestamp follows. It reads next to London-pinned
            service dates (`AttachTicketAction.tsx:67-68` on the merged
            /track/mine page) on purpose, not by oversight: a train's
            service date is a fact about the train and stays London wall-
            clock, like a departure board. Don't "fix" the inconsistency by
            converting either one. See
            docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md's
            Finding 1 and Recommendation. `LocalDateTime` is a client leaf
            so this component stays a Server Component -- only the one
            string needs the browser's zone, not the layout around it. */}
        <Text size="xs" c="dimmed">
          Added <LocalDateTime value={ticket.createdAt} />
        </Text>
      </Group>
    </Stack>
  );
}
