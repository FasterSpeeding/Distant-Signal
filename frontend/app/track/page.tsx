import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { TrackTrainForm } from '@/components/TrackTrainForm';

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead).
 *
 * Static rather than an async `generateMetadata()` despite this route
 * reading `searchParams`, for exactly the reason `/trains` gives: Next
 * hands `generateMetadata` the same `searchParams` the component below
 * gets, and `?ticketId=` here is one visitor's own saved ticket -- putting
 * it (or their `?origin=`) into a title would leak per-visitor state into
 * a cached, shared unfurl card for no gain whatsoever. The `<h1>`'s
 * subtitle below DOES vary with `attachTicketId`; this deliberately
 * describes the page's ordinary, shareable purpose instead, which is also
 * the branch an unfurler bot (never carrying either param) actually gets.
 *
 * Title matches the page's own `<h1>` ("Track a Train"). This route has no
 * nav label of its own to agree with -- it was dropped from the nav bar in
 * favour of `/trains` (see `app/layout.tsx`'s comment there) and is now
 * reached from `/trains`' manual-fallback link, `/stations/[crs]`, and
 * `TicketEntryForm` -- so the heading is the only name it has.
 *
 * The description's second half is the page's own default subtitle below,
 * near-verbatim, so the preview card and the page a visitor lands on say
 * the same thing. Its first half describes both ways the form can be
 * filled: picking a row from the departures `TrackTrainForm` loads for the
 * entered origin, or typing origin/scheduled departure (required) and
 * destination/operator (optional) by hand. Deliberately "upcoming
 * departures" rather than "the live departure board": that picker falls
 * back to the CIF-derived scheduled timetable at any station LDBWS has no
 * board for, and says so in its own copy. */
const METADATA_TITLE = 'Track a Train — Distant Signal';
const METADATA_DESCRIPTION =
  'Pin a specific train — picked from the upcoming departures at its origin station, or entered by hand — to see its live position, delay and next calling point as Network Rail reports it.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

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
