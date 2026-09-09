import { Stack, Title, Text } from '@mantine/core';
import { TrainSearchForm } from '@/components/TrainSearchForm';

/** `/trains` -- the primary train-discovery surface. Generalized from a
 * destination-first search into a calling-point-first one -- see
 * docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
 *
 * Ships ALONGSIDE `/track`, never replacing it: `/track`'s manual-entry
 * form is the honest fallback for every gap this search cannot close.
 *
 * Query params mirror `/track`'s own convention: `?station=` pre-fills the
 * required search key, `?origin=`/`?destination=` pre-fill the optional
 * filters (so a filtered search is a shareable link), and `?ticketId=`
 * carries a standalone ticket through so the row-level "Track this train"
 * action can attach it. */
export default async function TrainsPage({
  searchParams,
}: {
  searchParams: Promise<{
    station?: string | string[];
    origin?: string | string[];
    destination?: string | string[];
    date?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const { station, origin, destination, date, ticketId } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param -- fall back
  // to the first value rather than letting `.toUpperCase()` throw on an
  // array. Same handling as `app/track/page.tsx:10-13`.
  const stationParam = Array.isArray(station) ? station[0] : station;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const dateParam = Array.isArray(date) ? date[0] : date;
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Find a Train</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find the train your saved ticket is for — it'll be attached automatically once you track it."
          : 'Search the whole network by any station a train calls at. Open any result for its live status, or track it to get updates.'}
      </Text>
      <TrainSearchForm
        initialStation={stationParam?.toUpperCase()}
        initialOrigin={originParam?.toUpperCase()}
        initialDestination={destinationParam?.toUpperCase()}
        initialDate={dateParam}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );
}
