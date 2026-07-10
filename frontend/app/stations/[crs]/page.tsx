import { Stack, Title, Text, Divider, Group } from '@mantine/core';
import { getStopPointDisruption, getPreferences } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { DisruptionDetail } from '@/components/DisruptionDetail';
import { PinToggle } from '@/components/PinToggle';

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
      {reports.map((report) => (
        <div key={report.id}>
          <Divider my="sm" />
          <Text fw={600}>{report.name}</Text>
          {report.lineStatuses.map((status, i) => (
            <Stack key={i} gap="xs">
              <StatusBadge severity={status.statusSeverity} />
              <Text>{status.reason}</Text>
              {status.disruption && <DisruptionDetail disruption={status.disruption} />}
            </Stack>
          ))}
        </div>
      ))}
    </Stack>
  );
}
