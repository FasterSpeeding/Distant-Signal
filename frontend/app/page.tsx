import { Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import { getLineStatusForMode, getPreferences, getStopPointDisruption } from '@/lib/api';
import { LineStatusCard } from '@/components/LineStatusCard';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { severityRank } from '@/lib/severity';
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
  for (const report of reports) {
    const stats = report.lineStatuses.find((status) => status.sampleStats)?.sampleStats;
    if (stats) return stats;
  }
  return undefined;
}

export default async function DashboardPage() {
  const preferences = await getPreferences();

  const allReports = await getLineStatusForMode('national-rail');
  const pinnedLineReports = allReports.filter((report) => preferences.pinnedLines.includes(report.id));

  const pinnedStationEntries = await Promise.all(
    preferences.pinnedStations.map(async (crs) => ({
      crs,
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
            {pinnedStationEntries.map(({ crs, reports }) => {
              const stats = sampleStatsAcrossReports(reports);
              const cancelledPct = stats && stats.total > 0 ? Math.round((stats.cancelled / stats.total) * 100) : null;
              return (
                <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                  <Card withBorder>
                    <Stack gap={4}>
                      <Group justify="space-between">
                        <Text fw={600}>{crs}</Text>
                        <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                      </Group>
                      {stats && (
                        <Text size="xs" c="dimmed">
                          Avg delay {stats.avgDelayMinutes.toFixed(1)} min · {cancelledPct}% cancelled
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
