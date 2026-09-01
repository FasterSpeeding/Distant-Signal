import { Stack, Title, Text } from '@mantine/core';
import { TrackTrainForm } from '@/components/TrackTrainForm';

export default async function TrackPage({
  searchParams,
}: {
  searchParams: Promise<{ origin?: string | string[]; ticketId?: string | string[] }>;
}) {
  const { origin, ticketId } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param (e.g.
  // `?origin=a&origin=b`) -- fall back to the first value rather than
  // letting `.toUpperCase()` throw on an array.
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  // Set by `TicketEntryForm`'s own "find or track the train this ticket is
  // for" link (Part A of the upload-first plan) -- a standalone ticket's
  // id, carried forward so `TrackTrainForm` can attach it automatically
  // once a pin is created here. A malformed/non-numeric value is treated
  // the same as absent, rather than passing NaN through.
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Track a Train</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find or track the train your saved ticket is for — it'll be attached automatically once you do."
          : 'Pin a specific train to see its live position, delay and next calling point as Network Rail reports it.'}
      </Text>
      <TrackTrainForm initialOrigin={originParam?.toUpperCase()} attachTicketId={attachTicketId} />
    </Stack>
  );
}
