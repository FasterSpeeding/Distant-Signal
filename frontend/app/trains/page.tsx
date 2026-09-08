import { Stack, Title, Text } from '@mantine/core';
import { TrainSearchForm } from '@/components/TrainSearchForm';

/** `/trains` -- the primary train-discovery surface
 * (docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4).
 *
 * Ships ALONGSIDE `/track`, never replacing it: `/track`'s manual-entry
 * form is the honest fallback for every gap this search cannot close (a
 * station outside the CIF-derived data, a train past the per-destination
 * cap, a same-day amendment), and both `/stations/[crs]`'s "Track a train
 * from here" link and `TicketEntryForm`'s standalone-ticket flow still
 * point at it unchanged. See §4's explicit "do not delete or hide /track".
 *
 * Query params mirror `/track`'s own convention (`app/track/page.tsx`):
 * `?destination=`/`?origin=` pre-fill the filters (so a filtered search is
 * a shareable link -- the design doc's §7 Open Question 5, resolved
 * affirmatively because it costs one prop each), and `?ticketId=` carries a
 * standalone ticket through so the row-level "Track this train" action can
 * attach it, giving this page full parity with `/track?ticketId=...`. */
export default async function TrainsPage({
  searchParams,
}: {
  searchParams: Promise<{
    destination?: string | string[];
    origin?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const { destination, origin, ticketId } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param (e.g.
  // `?destination=a&destination=b`) -- fall back to the first value rather
  // than letting `.toUpperCase()` throw on an array. Same handling as
  // `app/track/page.tsx:10-13`.
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Find a Train</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find the train your saved ticket is for — it'll be attached automatically once you track it."
          : 'Search the whole network by where a train is going. Open any result for its live status, or track it to get updates.'}
      </Text>
      <TrainSearchForm
        initialDestination={destinationParam?.toUpperCase()}
        initialOrigin={originParam?.toUpperCase()}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );
}
