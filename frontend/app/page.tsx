import { Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import { getLineStatusForMode, getPreferences, getStopPointDisruption } from '@/lib/api';
import { LineStatusCard } from '@/components/LineStatusCard';
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
          <Link href="/lines" style={{ textDecoration: 'none' }}>
            <Text c="var(--mantine-color-anchor)">Browse all lines</Text>
          </Link>
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
          <Link href="/stations" style={{ textDecoration: 'none' }}>
            <Text c="var(--mantine-color-anchor)">Look up a station</Text>
          </Link>
        </Group>
        {pinnedStationEntries.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any stations yet. <Link href="/stations">Look up a station</Link> to pin one.
          </Text>
        ) : (
          <Stack gap="xs">
            {pinnedStationEntries.map(({ crs, reports }) => (
              <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                <Card withBorder>
                  <Group justify="space-between">
                    <Text fw={600}>{crs}</Text>
                    <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                  </Group>
                </Card>
              </Link>
            ))}
          </Stack>
        )}
      </Stack>
    </Stack>
  );
}
