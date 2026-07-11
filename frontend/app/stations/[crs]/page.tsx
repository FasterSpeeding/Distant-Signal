import { Stack, Title, Text, Group, Divider } from '@mantine/core';
import { getStopPointDisruption, getPreferences } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { PinToggle } from '@/components/PinToggle';
import { worstStatus } from '@/lib/severity';

export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;
  const [reports, preferences] = await Promise.all([getStopPointDisruption(crs), getPreferences()]);

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Disruptions at {crs}</Title>
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
            <IssueList statuses={report.lineStatuses} />
          </Stack>
        );
      })}
    </Stack>
  );
}
