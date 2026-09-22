import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { TrainSearchForm } from '@/components/TrainSearchForm';

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead).
 *
 * Static rather than an async `generateMetadata()` despite this route
 * reading `searchParams`, for the same reason `/incidents` gives: the
 * params here pre-fill a search form, and `?ticketId=` in particular is
 * one visitor's own ticket -- a title varying on it would leak per-visitor
 * state into a cached preview card for no gain. The `<h1>`'s copy below
 * does vary with `attachTicketId`; this deliberately describes the page's
 * ordinary, shareable purpose instead.
 *
 * Title matches the page's own `<h1>` ("Find a Train"), which is also the
 * nav label for this route.
 *
 * "another station along its route" is deliberately generic rather than
 * spelling out the ordering rule in this short SEO/OG blurb: `stops_at`
 * requires that named station to fall LATER in the journey than `station`
 * (2026-09-22; see `crates/api/src/data/queries.rs`, which spells the rule
 * out in full, and `TrainSearchForm`'s own field description). The wording
 * here is still accurate under that rule -- every match genuinely is
 * "another station along its route" -- it just does not additionally claim
 * the ordering, which belongs in the field's own, more detailed
 * description rather than this page-level summary. */
const METADATA_TITLE = 'Find a Train — Distant Signal';
const METADATA_DESCRIPTION =
  'Search scheduled UK trains by any station they call at, narrowing by origin, another station along its route, and date. Open any result for its live status, or track it to get updates.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

/** `/trains` -- the primary train-discovery surface. Generalized from a
 * destination-first search into a calling-point-first one -- see
 * docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md
 * and docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md.
 *
 * Ships ALONGSIDE `/track`, never replacing it: `/track`'s manual-entry
 * form is the honest fallback for every gap this search cannot close.
 *
 * Query params mirror `/track`'s own convention: `?station=` pre-fills the
 * required search key, `?origin=`/`?stops_at=`/`?date=`/`?from=`/`?to=`/
 * `?arrival_from=`/`?arrival_to=` pre-fill the optional filters (so a
 * filtered search is a shareable link, and so this /trains history entry's
 * own URL can restore what was last searched -- see
 * docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md),
 * and `?ticketId=` carries a standalone ticket through so the row-level
 * "Track this train" action can attach it. */
export default async function TrainsPage({
  searchParams,
}: {
  searchParams: Promise<{
    station?: string | string[];
    origin?: string | string[];
    stops_at?: string | string[];
    date?: string | string[];
    from?: string | string[];
    to?: string | string[];
    arrival_from?: string | string[];
    arrival_to?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const {
    station,
    origin,
    stops_at: stopsAt,
    date,
    from,
    to,
    arrival_from: arrivalFrom,
    arrival_to: arrivalTo,
    ticketId,
  } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param -- fall back
  // to the first value rather than letting `.toUpperCase()` throw on an
  // array. Same handling as `app/track/page.tsx:10-13`. `stops_at` is a
  // single-station filter now (see TrainSearchForm's own doc comment), so
  // it collapses to the first entry exactly like every other param here.
  const stationParam = Array.isArray(station) ? station[0] : station;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const stopsAtParam = Array.isArray(stopsAt) ? stopsAt[0] : stopsAt;
  const dateParam = Array.isArray(date) ? date[0] : date;
  // `from`/`to`/`arrival_from`/`arrival_to` are time-of-day strings
  // (`"HH:MM"`), not station codes -- unlike `station`/`origin`/`stops_at`
  // below, these are never `.toUpperCase()`d.
  const fromParam = Array.isArray(from) ? from[0] : from;
  const toParam = Array.isArray(to) ? to[0] : to;
  const arrivalFromParam = Array.isArray(arrivalFrom) ? arrivalFrom[0] : arrivalFrom;
  const arrivalToParam = Array.isArray(arrivalTo) ? arrivalTo[0] : arrivalTo;
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
        initialStopsAt={stopsAtParam?.toUpperCase()}
        initialDate={dateParam}
        initialFrom={fromParam}
        initialTo={toParam}
        initialArrivalFrom={arrivalFromParam}
        initialArrivalTo={arrivalToParam}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );
}
