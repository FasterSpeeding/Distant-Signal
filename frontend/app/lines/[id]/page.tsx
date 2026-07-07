import { notFound } from 'next/navigation';
import { Stack, Title, Text, Divider } from '@mantine/core';
import { getLineStatus } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { DisruptionDetail } from '@/components/DisruptionDetail';

export default async function LineDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const reports = await getLineStatus([id], true);

  if (reports.length === 0) {
    notFound();
  }

  const report = reports[0];

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>{report.name}</Title>
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      {report.lineStatuses.map((status, i) => (
        <div key={i}>
          <Divider my="sm" />
          <Stack gap="xs">
            <StatusBadge severity={status.statusSeverity} />
            <Text>{status.reason}</Text>
            <Text size="sm" c="dimmed">
              Data quality: {status.dataQuality}
            </Text>
            {status.disruption && <DisruptionDetail disruption={status.disruption} />}
          </Stack>
        </div>
      ))}
    </Stack>
  );
}
