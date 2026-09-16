import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { StationSearchForm } from './StationSearchForm';

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead). This one has
 * no params of any kind to vary on, so a static export is the only shape
 * that makes sense here.
 *
 * Title matches the page's own `<h1>` ("Station Disruption Lookup") rather
 * than the shorter nav label ("Station Lookup"), so the tab title and the
 * heading a visitor lands on agree -- the same rule `/incidents` and
 * `/trains` follow.
 *
 * The description reaches past this page into what a result actually shows
 * (`app/stations/[crs]/page.tsx`: disruptions, live departures, per-operator
 * sample stats, and accessibility & facilities) rather than only restating
 * the search box, because "search for a station" alone says nothing about
 * why a reader would want to. Keep it in step with that page if its
 * sections change. */
const METADATA_TITLE = 'Station Disruption Lookup — Distant Signal';
const METADATA_DESCRIPTION =
  'Look up any UK station by name or CRS code for the disruptions affecting lines through it, its live departures, per-operator punctuality and its accessibility & facilities.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

export default function StationSearchPage() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Station Disruption Lookup</Title>
      <Text c="dimmed">
        Search by station name or CRS code to see disruptions affecting lines through it.
      </Text>
      <StationSearchForm />
    </Stack>
  );
}
