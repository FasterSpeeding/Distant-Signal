'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { severityRank } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

function worstStatus(report: LineStatusReport) {
  if (report.lineStatuses.length === 0) {
    return { statusSeverity: 10, reason: '' };
  }
  // Rank by true severity (severe > mild > planned > informational > good),
  // not by the raw `statusSeverity` number, which is not monotonic with
  // actual severity — see lib/severity.ts.
  return report.lineStatuses.reduce((worst, current) =>
    severityRank(current.statusSeverity) > severityRank(worst.statusSeverity) ? current : worst,
  );
}

export function LineStatusCard({ report }: { report: LineStatusReport }) {
  const worst = worstStatus(report);
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
      </Stack>
    </Card>
  );
}
