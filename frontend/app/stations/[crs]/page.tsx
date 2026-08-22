import { Stack, Title, Text, Group, Divider } from '@mantine/core';
import { getStopPointDisruption, getPreferences, getStationName } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { PinToggle } from '@/components/PinToggle';
import { worstStatus } from '@/lib/severity';

// Falls back to `null` (rather than letting the page's error boundary take
// over) on any failure resolving the name — the heading should still show
// the bare CRS code the user actually asked for rather than an error page,
// since the disruption data itself is what matters most on this page.
async function resolveStationName(crs: string): Promise<string | null> {
  try {
    return await getStationName(crs);
  } catch {
    return null;
  }
}

export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;
  const [reports, preferences, stationName] = await Promise.all([
    getStopPointDisruption(crs),
    getPreferences(),
    resolveStationName(crs),
  ]);
  const heading = stationName ? `${stationName} (${crs})` : crs;

  // Stamped once for the whole page (all per-line IssueLists share it) so
  // their buckets don't depend on a `Date.now()` that differs between the
  // SSR pass and hydration. Fresh on every request (this route is dynamic)
  // and re-stamped by AutoRefresh.
  const now = Date.now();

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Disruptions at {heading}</Title>
        <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
      </Group>
      {reports.length === 0 && <Text c="dimmed">No disruptions affecting this station.</Text>}
      {reports.map((report) => {
        const worst = worstStatus(report);
        return (
          <Stack key={report.id} gap="sm">
            <Divider my="sm" />
            <Group justify="space-between">
              <Text fw={600}>{report.name}</Text>
              <StatusBadge severity={worst.statusSeverity} />
            </Group>
            <RepresentativeInfo statuses={report.lineStatuses} />
            <IssueList statuses={report.lineStatuses} now={now} />
          </Stack>
        );
      })}
    </Stack>
  );
}
