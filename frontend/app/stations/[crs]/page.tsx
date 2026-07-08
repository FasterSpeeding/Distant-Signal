import { Stack, Title, Text, Divider } from '@mantine/core';
import { getStopPointDisruption } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { DisruptionDetail } from '@/components/DisruptionDetail';

export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;
  const reports = await getStopPointDisruption(crs);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Disruptions at {crs}</Title>
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
