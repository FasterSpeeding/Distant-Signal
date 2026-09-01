import { Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import { getLineStatusForMode, getPreferences, getSession, getStationName, getStopPointDisruption } from '@/lib/api';
import { DISPLAYED_MODES_PARAM, MERGED_TFL_LINE_IDS } from '@/lib/modes';
import { LineStatusCard } from '@/components/LineStatusCard';
import { LoginPrompt } from '@/components/LoginPrompt';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { severityRank, worstStatus } from '@/lib/severity';
import { firstSampleStats, formatSampleSummary } from '@/lib/sampleStats';
import type { LineStatusReport } from '@/lib/types';

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

/** Anonymous-visitor "right now" widget data (§Home page redesign). Built
 * entirely from `allReports`, already fetched unconditionally by this page
 * for the pinned-lines section -- no new endpoint. Excludes
 * `MERGED_TFL_LINE_IDS` the same way `pinnedLineReports` already does,
 * since those ids are folded into their National Rail counterpart's row
 * everywhere a line list is built directly from reports rather than from
 * `/public/lines` -- counting them separately would double-count the same
 * real-world line. */
function notGoodServiceSummary(reports: LineStatusReport[]) {
  const affected = reports
    .filter((report) => !MERGED_TFL_LINE_IDS.includes(report.id))
    .filter((report) => severityRank(worstStatus(report).statusSeverity) > severityRank(10))
    // Same worst-first-then-alphabetical sort the pinned section already
    // uses (§Home page redesign: "same sort the pinned section already
    // does").
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });
  return { count: affected.length, worst: affected.slice(0, 5) };
}

export default async function DashboardPage() {
  // Same defensive fallback as app/layout.tsx and TicketPanel.tsx: an
  // auth-status glitch degrades to "treat as anonymous", not a broken
  // homepage. See docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
  // §Home page redesign.
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  const preferences = await getPreferences();

  // Every displayed mode, not just national-rail: a pinned TfL line would
  // otherwise be silently missing from "Your Lines".
  const allReports = await getLineStatusForMode(DISPLAYED_MODES_PARAM);

  if (!session.authenticated) {
    const { count, worst } = notGoodServiceSummary(allReports);
    return (
      <Stack p="lg" gap="xl">
        <Stack gap="xs">
          <Title order={1}>Distant Signal</Title>
          <Text c="dimmed">
            Live UK rail line status, train tracking, and Delay Repay support — pin the lines and
            stations you care about once you&apos;re logged in.
          </Text>
        </Stack>

        <Stack gap="md">
          <Title order={2}>Right now</Title>
          {count === 0 ? (
            <Text>Every line is running a Good Service.</Text>
          ) : (
            <>
              <Text>
                {count} line{count === 1 ? '' : 's'} not at Good Service right now:
              </Text>
              <Stack gap="xs">
                {worst.map((report) => (
                  <Link key={report.id} href={`/lines/${report.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                    <Card withBorder>
                      <Group justify="space-between">
                        <Text fw={600}>{report.name}</Text>
                        <StatusBadge severity={worstStatus(report).statusSeverity} />
                      </Group>
                    </Card>
                  </Link>
                ))}
              </Stack>
            </>
          )}
        </Stack>

        <Group gap="lg">
          <TextLink href="/lines">Browse all lines</TextLink>
          <TextLink href="/stations">Look up a station</TextLink>
          {/* Proactive, not reactive -- session is already in hand on this
              page (see this task's own Step 1), so there's no reason to
              wait for a failed pin click the way PinToggle does elsewhere.
              §Policy's Tier-2 "proactive where session is already fetched"
              refinement. */}
          <LoginPrompt verb="pin your lines and stations" />
        </Group>
      </Stack>
    );
  }

  // Logged-in branch: unchanged from today -- pinnedLineReports/
  // pinnedStationEntries computation and rendering exactly as on main.
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
    </Stack>
  );
}
