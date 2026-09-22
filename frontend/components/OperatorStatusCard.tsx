'use client';

import { Card, Group, Stack, Text } from '@mantine/core';
import { StatusBadge } from './StatusBadge';
import { LastUpdated } from './LastUpdated';
import { PinToggle } from './PinToggle';
import { formatOperatorSampleSummary } from '@/lib/operatorStats';
import type { OperatorSummary } from '@/lib/types';

/** Mirrors `LineStatusCard`'s shape (worst-status badge, reason, sample
 * summary, last-updated) -- deliberately NOT wrapped in a `Link` the way
 * `LineStatusCard` links to `/lines/{id}`: there is no
 * `/operators/[code]` detail page in this phase for a click to go to (see
 * docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 7). Used by both `/operators`' list page and the
 * homepage's "Your Operators" section -- the same reuse
 * `LineStatusCard` already gets across `/lines`... no, across the
 * homepage's "Your Lines" section (the one other place a status card like
 * this renders). */
export function OperatorStatusCard({
  operator,
  pinned,
  needsAccountHint = false,
}: {
  operator: OperatorSummary;
  pinned: boolean;
  needsAccountHint?: boolean;
}) {
  return (
    <Card withBorder shadow="sm" padding="lg">
      <Stack gap="xs">
        <Group justify="space-between" wrap="nowrap" gap="xs" data-card-title-row>
          <Text fw={600} lineClamp={2} style={{ minWidth: 0 }}>
            {operator.name}
          </Text>
          <StatusBadge severity={operator.worstSeverity} />
        </Group>
        <Text
          size="sm"
          c="dimmed"
          lineClamp={3}
          data-card-reason
          style={{ display: '-webkit-box', WebkitBoxOrient: 'vertical', WebkitLineClamp: 3, overflow: 'hidden' }}
        >
          {operator.reason || 'No current disruption reported.'}
        </Text>
        <Text size="xs" c="dimmed">
          {formatOperatorSampleSummary(operator)}
        </Text>
        <Group justify="space-between" align="center" wrap="nowrap">
          {operator.computedAt ? (
            <LastUpdated timestamp={operator.computedAt} />
          ) : (
            <Text size="xs" c="dimmed">
              &nbsp;
            </Text>
          )}
          <PinToggle
            kind="operator"
            id={operator.code}
            initiallyPinned={pinned}
            needsAccountHint={needsAccountHint}
          />
        </Group>
      </Stack>
    </Card>
  );
}
