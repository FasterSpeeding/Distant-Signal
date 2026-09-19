import { Group, Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { StationSearchForm } from './StationSearchForm';
import { TextLink } from '@/components/TextLink';

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
 * (`app/stations/[crs]/page.tsx`: disruptions, scheduled departures,
 * per-operator sample stats, and accessibility & facilities) rather than
 * only restating the search box, because "search for a station" alone says
 * nothing about why a reader would want to. Keep it in step with that page
 * if its sections change.
 *
 * Two words in it are load-bearing and must not be "tightened" into
 * something snappier: "scheduled" departures, because `StationTimetable`
 * disclaims in so many words that its rows are "from the scheduled
 * timetable, not live running information"; and "delay and cancellation
 * stats" rather than "punctuality", because that section is an LDBWS
 * SAMPLE (headed "Sample stats by operator", and a station can be outside
 * the sampling entirely), not a punctuality record. */
const METADATA_TITLE = 'Station Disruption Lookup — Distant Signal';
const METADATA_DESCRIPTION =
  'Look up any UK station by name or CRS code for the disruptions affecting lines through it, its scheduled departures, per-operator delay and cancellation stats, and its accessibility & facilities.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

/** review §3.5.13: the plan's own cheapest option for a page that was
 * otherwise empty below the search form -- a handful of major termini as
 * plain links, rather than the pinned-stations/localStorage-recents
 * options it also floats (both need per-user or per-browser state this
 * server component has no cheap way to reach; a future pass can add
 * either without touching this one). Not exhaustive and not meant to be --
 * eight of the UK's busiest interchange stations, so a reader with no
 * particular station in mind has somewhere to go besides a blank search
 * box. */
const MAJOR_STATIONS: { crs: string; name: string }[] = [
  { crs: 'KGX', name: 'London Kings Cross' },
  { crs: 'PAD', name: 'London Paddington' },
  { crs: 'WAT', name: 'London Waterloo' },
  { crs: 'VIC', name: 'London Victoria' },
  { crs: 'BHM', name: 'Birmingham New Street' },
  { crs: 'MAN', name: 'Manchester Piccadilly' },
  { crs: 'LDS', name: 'Leeds' },
  { crs: 'EDB', name: 'Edinburgh Waverley' },
];

export default function StationSearchPage() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Station Disruption Lookup</Title>
      <Text c="dimmed">
        Search by station name or CRS code to see disruptions affecting lines through it.
      </Text>
      <StationSearchForm />
      <Stack gap="xs">
        <Text size="sm" fw={500}>
          Or jump straight to a major station
        </Text>
        <Group gap="sm" wrap="wrap">
          {MAJOR_STATIONS.map((station) => (
            <TextLink key={station.crs} href={`/stations/${station.crs}`} underline="always">
              {station.name}
            </TextLink>
          ))}
        </Group>
      </Stack>
    </Stack>
  );
}
