'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { AiGeneratedBadge, ENRICHED_INCIDENT_SHORT_NOTE, isEnricherInfluenced } from './AiGeneratedBadge';
import { LastUpdated } from './LastUpdated';
import { UpcomingDisruptions } from './UpcomingDisruptions';
import { runsNoTrains, worstStatus } from '@/lib/severity';
import { representativeStatus, formatSampleSummary } from '@/lib/sampleStats';
import type { LineStatusReport } from '@/lib/types';

/** `showUpdated={false}` where the page states one "Updated" time for
 * every card (`/status`). */
export function LineStatusCard({ report, showUpdated = true }: { report: LineStatusReport; showUpdated?: boolean }) {
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
        {/* LEG-16: the worst status's severity and reason may have been
            shaped by the incident enricher's LLM. */}
        {'disruption' in worst && isEnricherInfluenced(worst.disruption.source) && (
          <Group gap={4}>
            <AiGeneratedBadge note={ENRICHED_INCIDENT_SHORT_NOTE} />
          </Group>
        )}
        <Text size="xs" c="dimmed">
          {runsNoTrains(worst.statusSeverity) ? 'No trains running' : formatSampleSummary(representative)}
        </Text>
        {/* A note, not a status: the badge above never reflects it. */}
        <UpcomingDisruptions upcoming={report.upcoming} compact />
        {showUpdated && <LastUpdated timestamp={report.computedAt} />}
      </Stack>
    </Card>
  );
}
