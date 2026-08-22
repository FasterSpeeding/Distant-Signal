import { Stack, Title, Text, Divider } from '@mantine/core';
import { getLineStatus, getLineStatusHistory } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import { HistoryRangePicker } from './HistoryRangePicker';
import { formatDateTime } from '@/lib/dateFormat';

// Same lookup the detail page uses for its heading. This page's job is the
// date range, not the name, so a failed lookup (line deleted, catalogue
// hiccup, etc.) falls back to the raw id instead of taking the whole page
// down the way `notFound()` would on the detail page. An empty result is
// the same fallback rather than a thrown TypeError, and the failure is
// logged so a genuine bug in here doesn't vanish silently. Mirrors
// `resolveStationName` on the station page.
async function resolveLineName(id: string): Promise<string> {
  try {
    const [report] = await getLineStatus([id], false);
    return report?.name ?? id;
  } catch (err) {
    console.warn(`Could not resolve a name for line "${id}"; falling back to the id.`, err);
    return id;
  }
}

export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string }>;
}) {
  const { id } = await params;
  const { from, to } = await searchParams;

  const name = await resolveLineName(id);

  return (
    <Stack p="lg" gap="md">
      <TextLink href={`/lines/${id}`} underline="always">
        Back to line
      </TextLink>
      <Title order={1}>History: {name}</Title>
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
            {formatDateTime(entry.computedAt)}
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
