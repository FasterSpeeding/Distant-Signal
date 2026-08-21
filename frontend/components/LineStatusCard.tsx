'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { LastUpdated } from './LastUpdated';
import { worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

export function LineStatusCard({ report }: { report: LineStatusReport }) {
  const worst = worstStatus(report);
  const stats = report.lineStatuses.find((status) => status.sampleStats)?.sampleStats;
  const cancelledPct = stats && stats.total > 0 ? Math.round((stats.cancelled / stats.total) * 100) : null;
  return (
    <Card withBorder shadow="sm" padding="lg" component={Link} href={`/lines/${report.id}`}>
      <Stack gap="xs">
        <Group justify="space-between">
          <Text fw={600}>{report.name}</Text>
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
        <Text size="sm" c="dimmed">
          {worst.reason}
        </Text>
        {stats && (
          <Text size="xs" c="dimmed">
            Avg delay {stats.avgDelayMinutes.toFixed(1)} min · {cancelledPct}% cancelled
          </Text>
        )}
        <LastUpdated timestamp={report.computedAt} />
      </Stack>
    </Card>
  );
}
