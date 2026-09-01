'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { LastUpdated } from './LastUpdated';
import { worstStatus } from '@/lib/severity';
import { representativeStatus, formatSampleSummary } from '@/lib/sampleStats';
import type { LineStatusReport } from '@/lib/types';

export function LineStatusCard({ report }: { report: LineStatusReport }) {
  const worst = worstStatus(report);
  const representative = representativeStatus(report.lineStatuses);
  return (
    <Card withBorder shadow="sm" padding="lg" component={Link} href={`/lines/${report.id}`}>
      <Stack gap="xs">
        {/* `wrap="nowrap"` with the name allowed to shrink: with Group's
            default wrapping, a long line name pushed the badge onto its own
            line on some cards and not others, so a grid of cards had no
            consistent place to look for status. `data-wrap` mirrors the prop
            as a plain DOM attribute (Mantine itself only exposes it as the
            `--group-wrap` CSS var) so tests can assert on it directly. */}
        <Group justify="space-between" wrap="nowrap" gap="xs" data-card-title-row data-wrap="nowrap">
          <Text fw={600} lineClamp={2} style={{ minWidth: 0 }}>
            {report.name}
          </Text>
          {/* StatusBadge opts out of Mantine's ellipsis (see globals.css),
              so it holds its full label here. */}
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
        {/* Three lines, not ten. These reasons run to whole paragraphs of
            machine-assembled text; the card's job is "is this line OK", and
            the detail page carries the rest. `lineClamp` drives Mantine's own
            clamp styling; the inline `-webkit-line-clamp` is set explicitly
            too since Mantine wires the prop through a `--text-line-clamp`
            CSS var (relying on its stylesheet) rather than an inline
            property, and this makes the clamp verifiable without it. */}
        <Text
          size="sm"
          c="dimmed"
          lineClamp={3}
          data-card-reason
          style={{ display: '-webkit-box', WebkitBoxOrient: 'vertical', WebkitLineClamp: 3, overflow: 'hidden' }}
        >
          {worst.reason}
        </Text>
        <Text size="xs" c="dimmed">
          {formatSampleSummary(representative)}
        </Text>
        <LastUpdated timestamp={report.computedAt} />
      </Stack>
    </Card>
  );
}
