import { Stack, Title, Text, Group, Divider } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import {
  getStopPointDisruption,
  getPreferences,
  getStationName,
  getStationSampleStats,
  getStationAccessibility,
  getAllTocs,
  ApiNotFoundError,
} from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { StatusBadge } from '@/components/StatusBadge';
import { IssueList } from '@/components/IssueList';
import { PinToggle } from '@/components/PinToggle';
import { ShareButton } from '@/components/ShareButton';
import { StationAccessibilitySection } from '@/components/StationAccessibilitySection';
import { StationTimetable } from '@/components/StationTimetable';
import { TextLink } from '@/components/TextLink';
import { worstStatus, severityRank, severityLabel } from '@/lib/severity';
import { dedupeStationIssues } from '@/lib/stationIssues';
import { representativeStatus, formatSampleSummary } from '@/lib/sampleStats';
import type {
  LineStatusReport,
  Preferences,
  StationAccessibilityData,
  StationOperatorSampleStats,
} from '@/lib/types';

/** Three outcomes, not two. The previous version collapsed "there is no
 * such station" and "the name lookup failed" into a single `null`, so the
 * page could not tell them apart — and rendered a cheerful "No disruptions
 * affecting this station." for `/stations/ZZZ`, complete with a working pin
 * button. An unknown code must 404; a lookup that merely failed must still
 * keep falling back to the bare CRS, since the disruption data is what
 * this page is actually for. */
type StationLookup =
  | { outcome: 'found'; name: string }
  | { outcome: 'unknown' }
  | { outcome: 'unavailable' };

/** Every CRS code is exactly three letters, so a malformed one is answered
 * without troubling the API at all. */
const CRS_PATTERN = /^[A-Za-z]{3}$/;

// The exact shape getPreferences() already returns for a 401, named so the
// fallback below is typed as `Preferences` rather than inferred with
// `never[]` members. Per-user data fails closed during an outage (design
// spec Decision 5) instead of being stale-served.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [] };

async function lookupStation(crs: string): Promise<StationLookup> {
  if (!CRS_PATTERN.test(crs)) return { outcome: 'unknown' };
  try {
    const name = await getStationName(crs);
    return name === null ? { outcome: 'unknown' } : { outcome: 'found', name };
  } catch {
    return { outcome: 'unavailable' };
  }
}

/** A real, curated-catalogue distinction, not a network one:
 * `/StopPoint/{crs}/Disruption` now 404s when no line's `stations` list
 * covers this CRS at all (crates/api/src/routes/line_status.rs's
 * `get_stop_point_disruption`), which is a genuinely different fact from
 * "covered, and every covering line is Good Service" (`200 []`). By the
 * time this runs, `lookupStation` has already confirmed the CRS is a real,
 * known station -- so this 404 can only mean "not covered by our line-status
 * tracking," never "unknown station" (that's `notFound()`'s job, above).
 * `withStaleFallback` already rethrows `ApiNotFoundError` rather than
 * stale-serving it (see that function's own doc comment: it's "a
 * meaningful application state," not a connectivity blip), so catching it
 * here is exactly where that state should land. */
type StationDisruptions = { reports: LineStatusReport[]; coverage: 'covered' | 'none' };

async function fetchStationDisruptions(crs: string): Promise<StationDisruptions> {
  try {
    const reports = await withStaleFallback(`stopPointDisruption:${crs}`, () => getStopPointDisruption(crs));
    return { reports, coverage: 'covered' };
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return { reports: [], coverage: 'none' };
    }
    throw err;
  }
}

/** A separate, orthogonal coverage question from `StationDisruptions`
 * above -- LDBWS live-sampling coverage, not line-status-catalogue
 * coverage, so a station can independently be `covered`/`none` for
 * disruptions and `sampled`/`not-sampled` for stats. Structurally mirrors
 * `fetchStationDisruptions`'s own 404-vs-other-failure split. See
 * docs/superpowers/specs/2026-09-03-per-station-stats-design.md Decision 9. */
type StationSampleStatsResult =
  | { coverage: 'not-sampled' }
  | { coverage: 'sampled'; operatorStats: StationOperatorSampleStats[] };

async function fetchStationSampleStats(crs: string): Promise<StationSampleStatsResult> {
  try {
    const operatorStats = await withStaleFallback(`stationSampleStats:${crs}`, () => getStationSampleStats(crs));
    return { coverage: 'sampled', operatorStats };
  } catch (err) {
    if (err instanceof ApiNotFoundError) return { coverage: 'not-sampled' };
    throw err;
  }
}

/** A third, independent coverage question from the two above -- whether
 * this app has ever captured `stations` reference data for this CRS at all
 * (`404`, `'unavailable'`), vs. whether it has but none of the twelve
 * allowlisted accessibility keys were present (`200 {}`, `'empty'`). See
 * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
 * Correction 5 / Decision 9: those are two different, already-representable
 * database states (the column is `NOT NULL DEFAULT '{}'`, but a CRS can
 * have no `stations` row at all) and must stay visibly distinct in the UI,
 * not collapsed into one "no data" message.
 *
 * `'unavailable'` is defence in depth rather than a routinely-reached
 * state on this page: `lookupStation` above resolves the heading through
 * `getStationName`, which searches the same `stations` table, so a CRS with
 * no row there already `notFound()`s the whole page before this runs. It
 * remains reachable if the row disappears between the two reads, and the
 * distinct copy is what the spec asks for regardless -- don't "simplify" it
 * away on the grounds that it looks unreachable. */
type StationAccessibilityResult =
  | { coverage: 'unavailable' }
  | { coverage: 'empty' }
  | { coverage: 'present'; data: StationAccessibilityData };

async function fetchStationAccessibility(crs: string): Promise<StationAccessibilityResult> {
  try {
    const data = await withStaleFallback(`stationAccessibility:${crs}`, () => getStationAccessibility(crs));
    return Object.keys(data).length === 0 ? { coverage: 'empty' } : { coverage: 'present', data };
  } catch (err) {
    if (err instanceof ApiNotFoundError) return { coverage: 'unavailable' };
    throw err;
  }
}

/** Per-page Open Graph/Twitter/`<title>` metadata for a shared station
 * link. Reuses `lookupStation`/`fetchStationDisruptions` -- the exact same
 * helpers the page component calls -- so the resulting `fetch()` calls
 * (same URL/options) are deduped by Next's own request memoization within
 * this render, same reasoning as the equivalent, more detailed comment on
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata`. Mirrors the page
 * component's own `notFound()`-for-an-unknown-CRS handling. */
export async function generateMetadata({
  params,
}: {
  params: Promise<{ crs: string }>;
}): Promise<Metadata> {
  const { crs } = await params;

  const lookup = await lookupStation(crs);
  if (lookup.outcome === 'unknown') {
    notFound();
  }

  const heading = lookup.outcome === 'found' ? `${lookup.name} (${crs})` : crs;
  const title = `${heading} — Distant Signal`;

  const { reports, coverage } = await fetchStationDisruptions(crs);
  let description: string;
  if (coverage === 'none') {
    description = `${heading}: not currently covered by our line-status tracking.`;
  } else if (reports.length === 0) {
    description = `${heading}: no disruptions currently affecting this station.`;
  } else {
    const worst = reports.reduce(
      (acc, report) => {
        const candidate = worstStatus(report);
        return severityRank(candidate.statusSeverity) > severityRank(acc.statusSeverity) ? candidate : acc;
      },
      worstStatus(reports[0]),
    );
    description = `${heading}: ${severityLabel(worst.statusSeverity)} reported.`;
  }

  return {
    title,
    description,
    openGraph: { title, description, type: 'website' },
    twitter: { card: 'summary', title, description },
  };
}

export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;

  // Deliberately awaited before the disruption fetch rather than in
  // parallel with it: `getStationName` is an hour-cached reference lookup,
  // so the serialization costs nothing in the common case, and an unknown
  // code should 404 without ever asking for its (empty) disruption list.
  const lookup = await lookupStation(crs);
  if (lookup.outcome === 'unknown') {
    notFound();
  }

  const [{ reports, coverage }, preferences, sampleStatsResult, accessibilityResult, tocs] = await Promise.all([
    fetchStationDisruptions(crs),
    // Per-user, so it fails closed to "nothing pinned" (the shape a 401
    // already returns) rather than being stale-served -- design spec
    // Decision 5. The pin button reads as unpinned during an outage.
    getPreferences().catch(() => NO_PREFERENCES),
    fetchStationSampleStats(crs),
    fetchStationAccessibility(crs),
    // Hour-cached reference data used only to label operator rows in the
    // sample-stats section below; an empty list degrades to bare ATOC
    // codes rather than the whole page -- same pattern as
    // app/lines/page.tsx's own getAllTocs() call.
    getAllTocs().catch(() => []),
  ]);
  const heading = lookup.outcome === 'found' ? `${lookup.name} (${crs})` : crs;

  // Stamped once for the whole page (all per-line IssueLists share it) so
  // their buckets don't depend on a `Date.now()` that differs between the
  // SSR pass and hydration. Fresh on every request (this route is dynamic)
  // and re-stamped by AutoRefresh.
  const now = Date.now();

  const items = dedupeStationIssues(reports);
  // Worst first, then alphabetical — the previous order was whatever the
  // API iterated, which visibly differed between viewports on the same data.
  const orderedReports = [...reports].sort((a, b) => {
    const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
    return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
  });

  // Same code-to-name resolution AllLinesTable.tsx:81 already establishes,
  // falling back to the bare ATOC code when `tocs` (hour-cached, fed by an
  // independent poller from `station_samples`) has no match.
  const tocNameByCode = new Map(tocs.map((toc) => [toc.code, toc.name]));

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        {/* review §3.5.1: this page is 85% accessibility & facilities
            content with no way to jump to it, and an `<h1>` reading
            "Disruptions at X" for a page that is mostly NOT disruptions
            was itself part of the problem. The heading now matches
            `generateMetadata`'s own title form; the "On this page" row
            immediately below restores the page's own framing (what the
            page is FOR) that the old title carried, as navigation rather
            than as prose. Deliberately not a tabs rewrite -- Decision 8's
            placement (accessibility last) is unchanged, only the page's
            framing changes, which the 09-17 review itself recommends as
            the cheaper option that gets most of the benefit. */}
        <Title order={1}>{heading}</Title>
        <Group gap="md">
          <TextLink href={`/track?origin=${crs}`}>Track a train from here</TextLink>
          <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
          <ShareButton />
        </Group>
      </Group>

      <Group
        component="nav"
        aria-label="On this page"
        gap="sm"
        c="dimmed"
        style={{ fontSize: 'var(--mantine-font-size-sm)' }}
      >
        <TextLink href="#disruptions" underline="always" inline>
          Disruptions
        </TextLink>
        <Text span c="dimmed">
          ·
        </Text>
        <TextLink href="#stats" underline="always" inline>
          Stats
        </TextLink>
        <Text span c="dimmed">
          ·
        </Text>
        <TextLink href="#departures" underline="always" inline>
          Departures
        </TextLink>
        <Text span c="dimmed">
          ·
        </Text>
        <TextLink href="#accessibility" underline="always" inline>
          Accessibility &amp; facilities
        </TextLink>
      </Group>

      {/* Two different absences, said honestly rather than collapsed into
          one cheerful "no disruptions" that would just as easily describe a
          station we've never modelled at all -- the bug this task exists to
          fix (see fetchStationDisruptions's doc comment above). Wrapped with
          the reports list below in one `id="disruptions"` container -- the
          "On this page" row's own jump target (review §3.5.1). */}
      <Stack gap="md" id="disruptions">
        {coverage === 'none' && (
          <Text c="dimmed">This station isn&apos;t covered by our line-status tracking yet.</Text>
        )}
        {coverage === 'covered' && reports.length === 0 && (
          <Text c="dimmed">No disruptions affecting this station.</Text>
        )}

        {orderedReports.length > 0 && (
          <>
            <Divider />
            {/* Per-line attribution, once — replacing three full copies of
                the same filter block, tab bar and issue list. The headings
                are links now, which the review asked for and the previous
                plain `Text` headings weren't. */}
            <Stack gap="xs">
              {orderedReports.map((report) => {
                const representative = representativeStatus(report.lineStatuses);
                return (
                  <Group key={report.id} justify="space-between" wrap="nowrap" gap="sm">
                    <Stack gap={0} style={{ minWidth: 0 }}>
                      <TextLink href={`/lines/${report.id}`}>{report.name}</TextLink>
                      <Text size="xs" c="dimmed">
                        {formatSampleSummary(representative)}
                      </Text>
                    </Stack>
                    <StatusBadge severity={worstStatus(report).statusSeverity} />
                  </Group>
                );
              })}
            </Stack>
            <Divider />
            <IssueList items={items} now={now} subject="station" />
          </>
        )}
      </Stack>

      {/* Sample stats by operator -- an independent block from the
          disruption section above, keyed by LDBWS live-sampling coverage
          rather than line-status-catalogue coverage (Decision 9). Three
          honest states, mirroring the disruption section's own
          coverage-vs-empty split immediately above. */}
      <Divider />
      <Stack gap="xs" id="stats">
        <Title order={2} size="h4">
          Sample stats by operator
        </Title>
        {sampleStatsResult.coverage === 'not-sampled' && (
          <Text c="dimmed">This station isn&apos;t part of our live departure sampling.</Text>
        )}
        {sampleStatsResult.coverage === 'sampled' && sampleStatsResult.operatorStats.length === 0 && (
          <Text c="dimmed">No live departures currently recorded at this station.</Text>
        )}
        {sampleStatsResult.coverage === 'sampled' &&
          sampleStatsResult.operatorStats.map((entry) => (
            <Group key={entry.operator} justify="space-between" wrap="nowrap" gap="sm">
              <Text size="sm">{tocNameByCode.get(entry.operator) ?? entry.operator}</Text>
              <Text size="xs" c="dimmed">
                {formatSampleSummary(entry)}
              </Text>
            </Group>
          ))}
      </Stack>

      <Divider />
      <div id="departures">
        <StationTimetable crs={crs} />
      </div>

      {/* Accessibility & facilities -- a fourth independent block, keyed by
          a third, orthogonal coverage question again: whether this app has
          any `stations` reference row for this CRS at all. Deliberately
          headed "Accessibility & facilities" rather than bare
          "Accessibility", which in this codebase means WCAG (design spec
          Correction 4 / Decision 8). */}
      <Divider />
      <div id="accessibility">
        <StationAccessibilitySection result={accessibilityResult} />
      </div>
    </Stack>
  );
}
