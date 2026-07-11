'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

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
