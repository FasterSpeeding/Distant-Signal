import { Badge, Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import { getLineStatusForMode, getMyTrackedTrains, getPreferences, getStationName, getStopPointDisruption } from '@/lib/api';
import { DISPLAYED_MODES_PARAM, MERGED_TFL_LINE_IDS } from '@/lib/modes';
import { LineStatusCard } from '@/components/LineStatusCard';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { severityRank, worstStatus } from '@/lib/severity';
import { firstSampleStats, formatSampleSummary } from '@/lib/sampleStats';
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { LineStatusReport, TrackedTrainListItem } from '@/lib/types';

// See app/lines/[id]/page.tsx-adjacent history page and this repo's other
// dynamic routes for the same `revalidate = 0` rationale: without it,
// Next.js treats this route as eligible for static generation and tries to
// prerender it during `next build`, which fails since the `api` service
// only exists on the compose network at runtime.
export const revalidate = 0;

function worstSeverityAcrossReports(reports: LineStatusReport[]): number {
  let worst = 10; // Good Service
  for (const report of reports) {
    for (const status of report.lineStatuses) {
      if (severityRank(status.statusSeverity) > severityRank(worst)) {
        worst = status.statusSeverity;
      }
    }
  }
  return worst;
}

/** First `sampleStats` found across every status on every report — mirrors
 * `RepresentativeInfo`'s "first one found is representative" rationale,
 * extended across a station's several affected lines. */
function sampleStatsAcrossReports(reports: LineStatusReport[]) {
  return reports.map((r) => firstSampleStats(r.lineStatuses)).find(Boolean);
}

export default async function DashboardPage() {
  // Concurrent, independent fetches -- getMyTrackedTrains() has no data
  // dependency on preferences or line status (its own null-on-401 return is
  // the complete "not logged in" signal; no getSession() call needed here,
  // mirroring /track/mine's own established reasoning), so serializing it
  // after the other two would only add latency for no reason. Mirrors this
  // page's existing pinnedStationEntries Promise.all precedent below. Per
  // docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md
  // Decision 3.
  const [preferences, allReports, myTrackedTrains] = await Promise.all([
    getPreferences(),
    // Every displayed mode, not just national-rail: a pinned TfL line would
    // otherwise be silently missing from "Your Lines".
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
    getMyTrackedTrains(),
  ]);
  // The pinned set came out in whatever order `/Line/Mode/…/Status`
  // happened to return, which visibly differed between two captures minutes
  // apart. Worst first, then alphabetical: a dashboard should lead with
  // what needs attention, and must not reshuffle under the user.
  const pinnedLineReports = allReports
    .filter((report) => preferences.pinnedLines.includes(report.id) && !MERGED_TFL_LINE_IDS.includes(report.id))
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });

  const pinnedStationEntries = await Promise.all(
    preferences.pinnedStations.map(async (crs) => ({
      crs,
      // The station detail page already shows "London Kings Cross (KGX)";
      // there is no reason for the dashboard to show a bare code. Hour-cached
      // reference data (see `getStationName`), and a failure here falls back
      // to the code rather than taking the dashboard down.
      name: await getStationName(crs).catch(() => null),
      reports: await getStopPointDisruption(crs),
    })),
  );

  // null (not logged in) collapses to [] -- the same "hide entirely"
  // treatment a logged-in user with zero tracked trains gets (Decision 4 of
  // the design spec). slice(0, 5) of an already trackedAt-DESC-ordered
  // response is "5 most recently tracked" with no client-side re-sort
  // needed (Decision 1/3) -- the backend query is already ordered that way.
  const trackedTrains = (myTrackedTrains ?? []).slice(0, 5);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Group justify="space-between">
          <Title order={1}>Your Lines</Title>
          <TextLink href="/lines">Browse all lines</TextLink>
        </Group>
        {pinnedLineReports.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any lines yet. <Link href="/lines">Browse all lines</Link> to pin some.
          </Text>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {pinnedLineReports.map((report) => (
              <LineStatusCard key={report.id} report={report} />
            ))}
          </SimpleGrid>
        )}
      </Stack>

      <Stack gap="md">
        <Group justify="space-between">
          <Title order={2}>Your Stations</Title>
          <TextLink href="/stations">Look up a station</TextLink>
        </Group>
        {pinnedStationEntries.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any stations yet. <Link href="/stations">Look up a station</Link> to pin one.
          </Text>
        ) : (
          <Stack gap="xs">
            {pinnedStationEntries.map(({ crs, name, reports }) => {
              const stats = sampleStatsAcrossReports(reports);
              return (
                <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                  <Card withBorder>
                    <Stack gap={4}>
                      <Group justify="space-between">
                        <Text fw={600}>{name ? `${name} (${crs})` : crs}</Text>
                        <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                      </Group>
                      {stats && (
                        <Text size="xs" c="dimmed">
                          {formatSampleSummary(stats)}
                        </Text>
                      )}
                    </Stack>
                  </Card>
                </Link>
              );
            })}
          </Stack>
        )}
      </Stack>

      {trackedTrains.length > 0 && (
        <Stack gap="md">
          <Group justify="space-between">
            <Title order={2}>Your Tracked Trains</Title>
            <TextLink href="/track/mine">View all</TextLink>
          </Group>
          <Stack gap="xs">
            {trackedTrains.map((train) => (
              <TrackedTrainSummaryRow key={train.id} train={train} />
            ))}
          </Stack>
        </Stack>
      )}
    </Stack>
  );
}

// Home-page-local mirror of /track/mine's own row shape
// (frontend/app/track/mine/page.tsx's TrackedTrainListRow/RowStatusBadge/
// STATUS_LABELS) -- same fields, same resolutionStatus-vs-status+
// delayMinutes branching, same words and colors. Deliberately NOT imported
// from that file or extracted into a shared component: per the design
// spec's Testing/Explicitly-out-of-scope sections, that extraction is a
// reasonable but non-mandated implementation-time choice, and this page
// having no import dependency on /track/mine's file keeps this change
// scoped to one file.
function TrackedTrainSummaryRow({ train }: { train: TrackedTrainListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- same logic as /track/mine's own row. The
  // resolved-with-null-trainUid fallback is defensive: the backend's own
  // resolution invariant means this shouldn't happen, but this component
  // doesn't assume it.
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = train.pinDestinationCrs ? `${train.pinOriginCrs} → ${train.pinDestinationCrs}` : train.pinOriginCrs;

  return (
    <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder>
        <Stack gap={4}>
          <Group justify="space-between" wrap="nowrap">
            <Text fw={500}>{route}</Text>
            <TrackedTrainStatusBadge train={train} />
          </Group>
          <Text size="sm" c="dimmed">
            {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

// Short, human badge words -- copied verbatim from /track/mine's own
// STATUS_LABELS so the two pages never disagree about wording for the same
// underlying tokens. Falls back to the raw token itself for anything
// unlisted, so an unexpected value never disappears from the badge.
const STATUS_LABELS: Record<string, string> = {
  pending: 'Pending match',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

function TrackedTrainStatusBadge({ train }: { train: TrackedTrainListItem }) {
  // pending/unresolved show the resolution status itself -- no journey
  // status exists yet for either. Once resolved, the journey status plus a
  // delay badge takes over. No "active only" filter and no attempt to
  // distinguish a genuinely-finished journey from one that's merely gone
  // quiet -- per Decision 1/Finding 1 of the design spec, the backend can't
  // honestly support that distinction today.
  if (train.resolutionStatus !== 'resolved') {
    return (
      <Badge color={train.resolutionStatus === 'unresolved' ? 'red' : 'gray'} variant="light">
        {STATUS_LABELS[train.resolutionStatus] ?? train.resolutionStatus}
      </Badge>
    );
  }
  return (
    <Group gap={6} wrap="nowrap">
      {train.status && (
        <Badge color={train.status === 'cancelled' ? 'red' : 'gray'} variant="light">
          {STATUS_LABELS[train.status] ?? train.status}
        </Badge>
      )}
      {train.delayMinutes !== null && (
        <Badge color={train.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
          {train.delayMinutes > 0 ? `${train.delayMinutes}m late` : 'On time'}
        </Badge>
      )}
    </Group>
  );
}
