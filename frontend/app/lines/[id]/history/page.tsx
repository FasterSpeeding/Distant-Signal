import { Stack, Title, Text, Divider } from '@mantine/core';
import { getLineStatusHistory } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { HistoryRangePicker } from './HistoryRangePicker';

export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string }>;
}) {
  const { id } = await params;
  const { from, to } = await searchParams;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>History: {id}</Title>
      <HistoryRangePicker lineId={id} />
      {from && to && <HistoryResults id={id} from={from} to={to} />}
    </Stack>
  );
}

async function HistoryResults({ id, from, to }: { id: string; from: string; to: string }) {
  const entries = await getLineStatusHistory(id, from, to);

  if (entries.length === 0) {
    return <Text c="dimmed">No history entries in that range.</Text>;
  }

  return (
    <Stack gap="xs">
      {entries.map((entry, i) => (
        <div key={i}>
          <Divider my="sm" />
          <Text size="sm" c="dimmed">
            {new Date(entry.computedAt).toLocaleString()}
          </Text>
          {entry.lineStatuses.map((status, j) => (
            <Stack key={j} gap={4}>
              <StatusBadge severity={status.statusSeverity} />
              <Text size="sm">{status.reason}</Text>
            </Stack>
          ))}
        </div>
      ))}
    </Stack>
  );
}
