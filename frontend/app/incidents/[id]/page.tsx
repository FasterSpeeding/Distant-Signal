import { notFound } from 'next/navigation';
import { Badge, Divider, Group, Stack, Text, Title } from '@mantine/core';
import { ApiNotFoundError, getIncident } from '@/lib/api';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import { TextLink } from '@/components/TextLink';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentDetail, IncidentHistoryEntry, ValidityPeriod } from '@/lib/types';

// Same rationale as every dynamic `[param]` route in this app: without
// this, `next build` may try to prerender against a database that only
// exists on the compose network at runtime. (Note: `/lines/[id]/page.tsx`
// and `/stations/[crs]/page.tsx` — the two structurally closest existing
// pages — do NOT declare this explicitly and still render dynamically, so
// this may be a no-op in practice; added anyway for explicitness, matching
// `/lines/[id]/history/page.tsx`'s and the dashboard's convention.)
export const revalidate = 0;

function formatValidityPeriod(period: ValidityPeriod): string {
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}

/** Which of a history entry's fields differ from the entry immediately
 * after it in the (newest-first) list — a short textual diff summary
 * rather than a full field dump every time, since most consecutive
 * snapshots differ in only one or two fields. `older` is `undefined` for
 * the oldest entry (nothing to diff against — it's the incident's
 * first-seen snapshot). */
function describeChanges(entry: IncidentHistoryEntry, older: IncidentHistoryEntry | undefined): string {
  if (!older) return 'First seen';
  const changes: string[] = [];
  if (entry.summary !== older.summary) changes.push('summary changed');
  if (entry.description !== older.description) changes.push('description changed');
  if (entry.priority !== older.priority) changes.push(`priority changed from ${older.priority} to ${entry.priority}`);
  if (JSON.stringify(entry.validityPeriods) !== JSON.stringify(older.validityPeriods)) changes.push('validity changed');
  if (entry.isPlanned !== older.isPlanned) changes.push(`isPlanned changed to ${entry.isPlanned}`);
  if (entry.isCleared !== older.isCleared) changes.push(`isCleared changed to ${entry.isCleared}`);
  return changes.length > 0 ? changes.join(', ') : 'Re-confirmed, no change';
}

export default async function IncidentDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;

  let incident: IncidentDetail;
  try {
    incident = await getIncident(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Group gap="sm">
        <Title order={1}>{incident.summary}</Title>
        <Badge color={incident.isPlanned ? 'blue' : 'orange'}>{incident.isPlanned ? 'Planned Work' : 'Real-Time'}</Badge>
      </Group>

      <div dangerouslySetInnerHTML={{ __html: sanitizeDescription(incident.description) }} />

      {incident.affectedStations.length > 0 && (
        <Group gap="xs">
          {incident.affectedStations.map((crs) => (
            <Badge key={crs} variant="outline" color="gray">
              {crs}
            </Badge>
          ))}
        </Group>
      )}

      <Stack gap={4}>
        <Text fw={500}>Validity</Text>
        {incident.validityPeriods.map((period, i) => (
          <Text key={i} size="sm" c="dimmed">
            {formatValidityPeriod(period)}
          </Text>
        ))}
      </Stack>

      <Divider />

      <Stack gap={4}>
        <Text fw={500}>Currently affects</Text>
        {incident.currentlyAffectsLines.length === 0 ? (
          <Text size="sm" c="dimmed">
            Not currently reported on any tracked line.
          </Text>
        ) : (
          <Group gap="md">
            {incident.currentlyAffectsLines.map((line) => (
              <TextLink key={line.id} href={`/lines/${line.id}`}>
                {line.name}
              </TextLink>
            ))}
          </Group>
        )}
      </Stack>

      <Divider />

      <Stack gap="xs">
        <Text fw={500}>History</Text>
        {incident.history.map((entry, i) => (
          <Stack key={i} gap={2}>
            <Text size="sm">{formatDateTime(entry.recordedAt)}</Text>
            <Text size="sm" c="dimmed">
              {describeChanges(entry, incident.history[i + 1])}
            </Text>
          </Stack>
        ))}
      </Stack>

      <Divider />

      <Stack gap={2}>
        <Text size="xs" c="dimmed">
          First seen: {formatDateTime(incident.firstSeenAt)}
        </Text>
        <Text size="xs" c="dimmed">
          Last fetched: {formatDateTime(incident.fetchedAt)}
        </Text>
      </Stack>
    </Stack>
  );
}
