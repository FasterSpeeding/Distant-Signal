import { Stack, Title, Text, Group, Divider } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getStopPointDisruption, getPreferences, getStationName, ApiNotFoundError } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { StatusBadge } from '@/components/StatusBadge';
import { IssueList } from '@/components/IssueList';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { worstStatus, severityRank } from '@/lib/severity';
import { dedupeStationIssues } from '@/lib/stationIssues';
import { representativeStatus, formatSampleSummary } from '@/lib/sampleStats';
import type { LineStatusReport, Preferences } from '@/lib/types';

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

  const [{ reports, coverage }, preferences] = await Promise.all([
    fetchStationDisruptions(crs),
    // Per-user, so it fails closed to "nothing pinned" (the shape a 401
    // already returns) rather than being stale-served -- design spec
    // Decision 5. The pin button reads as unpinned during an outage.
    getPreferences().catch(() => NO_PREFERENCES),
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

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Disruptions at {heading}</Title>
        <Group gap="md">
          <TextLink href={`/track?origin=${crs}`}>Track a train from here</TextLink>
          <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
        </Group>
      </Group>

      {/* Two different absences, said honestly rather than collapsed into
          one cheerful "no disruptions" that would just as easily describe a
          station we've never modelled at all -- the bug this task exists to
          fix (see fetchStationDisruptions's doc comment above). */}
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
  );
}
