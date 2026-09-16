import { Group, Stack, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllLines, getAllTocs, getLineStatusForMode, getPreferences } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
import type { Preferences } from '@/lib/types';
import { TextLink } from '@/components/TextLink';
import { AllLinesTable } from './AllLinesTable';

export const revalidate = 0;

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead). This route
 * takes no params of any kind -- no dynamic segment, no `searchParams` --
 * so a static export is the only shape that makes sense here.
 *
 * Title matches the page's own `<h1>` ("All Lines"), which is also this
 * route's nav label, so the tab title and the heading a visitor lands on
 * agree -- the same rule `/incidents`, `/trains` and `/stations` follow.
 *
 * Three phrases in the description are load-bearing and must not be
 * "tightened":
 *
 * - "National Rail and TfL lines" -- `GET /public/lines`
 *   (`crates/api/src/routes/lines.rs`'s `list_lines`) concatenates the
 *   static line catalogue with `queries::tfl_line_summaries`, so the table
 *   genuinely carries both, not just heavy-rail routes.
 * - "your own custom lines once you're logged in" -- that same handler
 *   appends custom lines ONLY for an authenticated caller, and only that
 *   caller's own (a line merely shared with them through a group is
 *   deliberately excluded from this list; see the handler's own comment).
 *   An anonymous visitor -- which is every link-unfurler bot, none of
 *   which carry a session cookie -- sees none, hence the hedge rather
 *   than a flat promise.
 * - "where they're available" -- the Avg Delay and Cancelled cells render
 *   an em dash with a "why not" tooltip whenever a line has no stats
 *   (`AllLinesTable`'s `representative?.fullCoverageStats ??
 *   representative?.sampleStats`), which is normal, not an outage.
 *
 * Deliberately says nothing about the country filter: it is self-hiding
 * below two distinct countries and today every reachable row is GB (see
 * `AllLinesTable`'s `countryOptions`), so describing it would promise a
 * control nobody currently sees. */
const METADATA_TITLE = 'All Lines — Distant Signal';
const METADATA_DESCRIPTION =
  "Every National Rail and TfL line this app tracks, in one sortable table: each line's worst current status, plus its average delay and cancellation figures where they're available, filterable by operator — and your own custom lines alongside them once you're logged in.";

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

// The exact shape getPreferences() already returns for a 401, named so the
// fallback below is typed as `Preferences` rather than inferred with
// `never[]` members. Per-user data fails closed during an outage (design
// spec Decision 5) instead of being stale-served.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [] };

export default async function AllLinesPage() {
  const [lines, preferences, reports, tocs] = await Promise.all([
    withStaleFallback('allLines', () => getAllLines()),
    // Per-user, so it fails closed to "nothing pinned" (the shape a 401
    // already returns) rather than being stale-served -- design spec
    // Decision 5.
    getPreferences().catch(() => NO_PREFERENCES),
    // Deliberately the same cache key as app/page.tsx: it is the same
    // request, so the two pages should share one entry.
    withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
      getLineStatusForMode(DISPLAYED_MODES_PARAM),
    ),
    // Hour-cached reference data used only to label rows; an empty list
    // degrades the table's operator column rather than the whole page.
    getAllTocs().catch(() => []),
  ]);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Group justify="space-between" align="baseline">
          <Title order={1}>All Lines</Title>
          <Group gap="md">
            <TextLink href="/incidents">Incident Archive</TextLink>
            <TextLink href="/lines/new">New custom line</TextLink>
          </Group>
        </Group>
        <AllLinesTable lines={lines} reports={reports} pinnedLineIds={preferences.pinnedLines} tocs={tocs} />
      </Stack>
    </Stack>
  );
}
