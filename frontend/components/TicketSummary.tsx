import { Stack, Text } from '@mantine/core';
import type { TrackedTrainTicket, TicketListItem } from '@/lib/types';

/** The "operator — ticket type" / "origin → destination" row renderer,
 * shared by `TicketPanel.tsx` (one tracked train's own tickets) and the
 * merged `app/track/mine/page.tsx` (both a train's attached tickets and
 * its own standalone-tickets section) -- extracted out of `TicketPanel.tsx`,
 * where it was previously a private, unexported function, so both can
 * reuse it rather than duplicating ticket-row rendering. `Pick<...>` keeps
 * the prop narrow: this component only ever reads these four fields, from
 * either wire shape. */
export function TicketSummary({
  ticket,
}: {
  ticket: Pick<TrackedTrainTicket | TicketListItem, 'operator' | 'ticketType' | 'originCrs' | 'destinationCrs'>;
}) {
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
