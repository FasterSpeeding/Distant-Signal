import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { TrainsLookup } from '@/components/TrainsLookup';
import { parseStopFilterParams } from '@/lib/trainStopFilters';
import { getAllTocs, getTrainSearchDates, getTrainSearchOperators } from '@/lib/api';
import { OWN_PREVIEW_IMAGE, pageMetadata } from '@/lib/pageMetadata';
import { trainsViewFor } from '@/lib/trainSearch';

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
 * state into a cached preview card for no gain. The canonical URL is the
 * bare `/trains`, whatever the query string.
 *
 * Title matches the page's own `<h1>` and its nav label, "Trains"
 * (`lib/navLinks.ts`'s `TRAINS_DESTINATION`, docs/style-guide.md "Tone"). */
const METADATA_TITLE = 'Trains';
const METADATA_DESCRIPTION =
  'Timetables for every station: departures, arrivals, trains between two stations, or one train by headcode or UID.';

export const metadata: Metadata = pageMetadata(METADATA_TITLE, METADATA_DESCRIPTION, {
  ...OWN_PREVIEW_IMAGE,
  path: '/trains',
});

type TrainsSearchParams = Record<string, string | string[] | undefined>;

/** The page's `searchParams` as a `URLSearchParams`, keeping every value of
 * a repeated param, in order. */
function toUrlSearchParams(raw: TrainsSearchParams): URLSearchParams {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(raw)) {
    if (value === undefined) continue;
    for (const entry of Array.isArray(value) ? value : [value]) params.append(key, entry);
  }
  return params;
}

/** `/trains` -- the Trains section: the whole network's timetable, for
 * seeing schedules and finding a specific train, with no tracking or
 * journey steps in the way (2026-10-30; until then this was "Find a
 * train", reached from the Journeys page and marked as part of Journeys
 * in the nav). Three views (`TrainsLookup`): a station's departures or
 * arrivals, the trains between two stations, or one train by headcode or
 * UID. Every result opens the train's own page (`/train/[uid]/[date]`),
 * where tracking it is a secondary action.
 *
 * The query string is the view (see `TRAIN_URL_PARAMS`), so each is a
 * shareable link:
 *
 * - `?station=RDG[&board=arrivals]`, `?from=RDG&to=PAD` or
 *   `?headcode=1S00`, with `date`, `time`/`until` and the filters;
 * - `?uid=W12345[&date=]` redirects to that train's page (a 307: with no
 *   date it depends on today), and a link in the old shape (`stops_at`, or
 *   `from`/`to` as times) gets a permanent redirect to the same search in
 *   today's names; `proxy.ts` sends both (`trainsRedirect`), before this
 *   page renders;
 * - `?ticketId=` (the Journeys page's "find the train this ticket is for")
 *   is carried through, and only then do rows offer "Track this train".
 *
 * Specs: docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md,
 * 2026-09-09-stops-at-search-filter-design.md and
 * 2026-09-22-train-search-state-persistence-design.md. */
export default async function TrainsPage({ searchParams }: { searchParams: Promise<TrainsSearchParams> }) {
  const params = toUrlSearchParams(await searchParams);
  // Next.js supplies a `string[]` for a repeated query param: the first
  // value is used. Station codes are uppercased; times, dates and
  // operator codes are read as they are by the form.
  const first = (key: string) => params.get(key) ?? undefined;
  const code = (key: string) => first(key)?.toUpperCase();
  const view = trainsViewFor(params);
  const between = view === 'between';
  const ticketIdParam = first('ticketId');
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;
  // Names each result row's operator, from the same hour-cached TOC list
  // `app/stations/[crs]/page.tsx` fetches. If the list fails to load (or
  // comes back empty) the rows carry no operator at all, rather than a
  // bare ATOC code. The date picker's bounds come from
  // `getTrainSearchDates` (a failure falls back to a week either side of
  // today), and the Operator field also offers the codes the timetable
  // carries that the TOC list does not name (`getTrainSearchOperators`).
  const [tocs, searchDates, searchOperators] = await Promise.all([
    getAllTocs().catch(() => []),
    getTrainSearchDates().catch(() => null),
    getTrainSearchOperators().catch(() => []),
  ]);
  const operatorNames = tocs.length > 0 ? Object.fromEntries(tocs.map((toc) => [toc.code, toc.name])) : undefined;
  const unnamedOperatorCodes = searchOperators
    .filter((operator) => operator.name === null && !(operatorNames && operator.code in operatorNames))
    .map((operator) => operator.code);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Trains</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find the train your saved ticket is for. It'll be attached automatically once you track it."
          : 'Timetables for the whole network. See what leaves or arrives at a station, the trains between two stations, or one train by its headcode or UID.'}
      </Text>
      <TrainsLookup
        initialView={view}
        initialCode={code('headcode') ?? code('uid')}
        searchForm={{
          initialBoard: first('board'),
          initialStation: code(between ? 'from' : 'station'),
          initialStopsAt: between ? code('to') : undefined,
          initialDate: first('date'),
          initialFrom: first('time'),
          initialTo: first('until'),
          initialArrivalFrom: first('arrival_from'),
          initialArrivalTo: first('arrival_to'),
          initialFromDayOffset: first('time_day_offset'),
          initialToDayOffset: first('until_day_offset'),
          initialArrivalFromDayOffset: first('arrival_from_day_offset'),
          initialArrivalToDayOffset: first('arrival_to_day_offset'),
          initialOperator: first('operator'),
          initialStopFilters: parseStopFilterParams(params),
          attachTicketId,
          operatorNames,
          unnamedOperatorCodes,
          searchDates,
        }}
      />
    </Stack>
  );
}
