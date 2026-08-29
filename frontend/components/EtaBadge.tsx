import { Badge, Group, Text, Tooltip } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { EtaSource } from '@/lib/types';

/** Renders nothing when there's no ETA at all (`etaNext` null) -- a
 * tracked train that hasn't been resolved yet, or has no current-state
 * row, has nothing to show here. When there IS an ETA, `etaSource` is
 * always shown as a distinct badge alongside the time, never collapsed
 * into one number -- extending this app's existing `dataQuality`
 * provenance-surfacing philosophy (`StatusBadge`/`LineStatus.dataQuality`)
 * to ETAs, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 3. */
export function EtaBadge({ etaNext, etaSource }: { etaNext: string | null; etaSource: EtaSource | null }) {
  if (!etaNext || !etaSource) return null;

  const label = etaSource === 'darwin-estimated' ? 'Live departure board' : 'Network Rail propagated';
  const tooltip =
    etaSource === 'darwin-estimated'
      ? 'Estimated from a live Darwin/National Rail Enquiries departure board sample at the origin station'
      : "Estimated by Network Rail's TRUST movement feed, propagated forward from the train's last reported delay";

  return (
    <Group gap={6} wrap="nowrap">
      <Text size="sm">ETA {formatTime(etaNext)}</Text>
      <Tooltip label={tooltip}>
        <Badge color={etaSource === 'darwin-estimated' ? 'teal' : 'gray'} variant="light">
          {label}
        </Badge>
      </Tooltip>
    </Group>
  );
}
