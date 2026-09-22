import { Group, Stack, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllLines, getAllTocs, getLineStatusForMode, getPreferences, getSession } from '@/lib/api';
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
 * Title matches the page's own `<h1>` ("All Lines"), so the tab title and
 * the heading a visitor lands on agree -- the same rule `/incidents`,
 * `/trains` and `/stations` follow. Here the `<h1>` also happens to be the
 * nav label; on `/stations` the two differ and the `<h1>` still wins (see
 * that page's own comment).
 *
 * Three phrases in the description are load-bearing and must not be
 * "tightened":
 *
 * - "National Rail and TfL line" -- `GET /public/lines`
 *   (`crates/api/src/routes/lines.rs`'s `list_lines`) concatenates the
 *   static line catalogue with a merge-filtered
 *   `queries::tfl_line_summaries` (its `!is_merged_into_nr_line` filter
 *   drops the TfL rows whose railway is already represented by an NR
 *   catalogue row, e.g. the Elizabeth line), so the table genuinely
 *   carries both networks, not just heavy-rail routes.
 * - "your own custom lines once you're logged in" -- that same handler
 *   appends custom lines ONLY for an authenticated caller, and only that
 *   caller's own (a line merely shared with them through a group is
 *   deliberately excluded from this list; see the handler's own comment).
 *   An anonymous visitor -- which is every link-unfurler bot, none of
 *   which carry a session cookie -- sees none, hence the hedge rather
 *   than a flat promise. That is also why the whole clause sits up front
 *   rather than at the end: unfurlers commonly truncate a description
 *   around 155-200 characters, and a cut landing between "your own custom
 *   lines" and "once you're logged in" would turn the hedge into exactly
 *   the flat promise it exists to avoid. Both halves are inside the first
 *   ~100 characters here; keep them there when rewording.
 * - "where available" -- the Avg Delay and Cancelled cells render an em
 *   dash for a line with no stats at all (`AllLinesTable`'s
 *   `representative?.fullCoverageStats ?? representative?.sampleStats`),
 *   with a "why not" tooltip where there is a representative status to
 *   explain it from and a bare dash where there isn't. Normal, not an
 *   outage.
 *
 * Deliberately says nothing about the country filter: it is self-hiding
 * below two distinct countries and today every reachable row is GB (see
 * `AllLinesTable`'s `countryOptions`), so describing it would promise a
 * control nobody currently sees. */
const METADATA_TITLE = 'All Lines — Distant Signal';
const METADATA_DESCRIPTION =
  "Every National Rail and TfL line this app tracks — plus your own custom lines once you're logged in — in one sortable, operator-filterable table: worst current status, average delay and cancellation figures where available.";

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
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };

export default async function AllLinesPage() {
  const [lines, preferences, reports, tocs, viewerIsAnonymous] = await Promise.all([
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
    // Task 3.4.13: only used to hint the pin star that pinning needs an
    // account. A failed session check degrades to "treat as anonymous" --
    // an extra hint shown to someone who is in fact logged in is harmless,
    // where hiding a real hint from an anonymous visitor is the failure
    // this feature exists to fix.
    getSession()
      .then((session) => !session.authenticated)
      .catch(() => true),
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
        <AllLinesTable
          lines={lines}
          reports={reports}
          pinnedLineIds={preferences.pinnedLines}
          tocs={tocs}
          viewerIsAnonymous={viewerIsAnonymous}
        />
      </Stack>
    </Stack>
  );
}
