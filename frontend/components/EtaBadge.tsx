import { Badge, Group, Text, Tooltip, VisuallyHidden } from '@mantine/core';
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
 * Decision 3.
 *
 * The non-Darwin badge used to read "NETWORK RAIL PROPAGATED" -- jargon
 * ("propagated") on the most-read line of both train pages (review §2.9).
 * The short label now says what it means in plain terms; the precise
 * technical description survives in two places for anyone who wants it:
 * the `Tooltip` (sighted, on hover/focus) and a `VisuallyHidden` span
 * (screen readers, unconditionally -- a `Tooltip`'s content isn't reliably
 * exposed to assistive tech without an explicit hover/focus, so this
 * doesn't depend on that). The hidden span is a sibling of the `Badge`,
 * not nested inside it, so the badge's own visible text stays a single
 * plain string rather than a string interleaved with hidden content. */
export function EtaBadge({ etaNext, etaSource }: { etaNext: string | null; etaSource: EtaSource | null }) {
  if (!etaNext || !etaSource) return null;

  const label = etaSource === 'darwin-estimated' ? 'Live departure board' : 'Estimate (Network Rail)';
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
      <VisuallyHidden>{tooltip}</VisuallyHidden>
    </Group>
  );
}
