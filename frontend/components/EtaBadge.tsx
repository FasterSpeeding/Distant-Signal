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
 * plain string rather than a string interleaved with hidden content.
 *
 * `mayHaveArrived` (Task 3.6.3): the present-tense "ETA {time}" badge
 * above reads as live/current, but once the server's own
 * `may_have_arrived` heuristic (`crates/api/src/data/journey.rs`) has
 * fired, that time is actually the STALE estimate that tripped it -- no
 * arrival report has been received for well over an hour in practice
 * (the heuristic's own threshold is 15 minutes, but a poll gap or a quiet
 * feed period can leave it considerably staler still by the time anyone
 * reads this). Showing "ETA 14:34" unchanged under a "may have arrived"
 * banner elsewhere on the page (`TrainJourney.tsx`'s `StatusMessage`)
 * contradicts that banner's own wording. This branch swaps the badge for
 * a single past-tense line instead, using the exact same `etaNext` value
 * (still the best time this app has for the terminus) and the same
 * `formatTime` convention every other network-time value on this page
 * uses -- not a new estimate, just honest tense. `etaSource` is not shown
 * here: once the estimate is flagged stale, which feed produced it is
 * secondary to the fact that nothing has confirmed it since.
 * `destinationCrs`/`destinationName` are optional and both nullable --
 * an unmatched schedule genuinely has neither -- so the line degrades to
 * "Was due 14:34 (no arrival report received)" rather than fabricating a
 * station. */
export function EtaBadge({
  etaNext,
  etaSource,
  mayHaveArrived = false,
  destinationCrs = null,
  destinationName = null,
}: {
  etaNext: string | null;
  etaSource: EtaSource | null;
  mayHaveArrived?: boolean;
  destinationCrs?: string | null;
  destinationName?: string | null;
}) {
  if (!etaNext) return null;

  if (mayHaveArrived) {
    const station = destinationName ?? destinationCrs;
    const dueLine = station ? `Was due at ${station} ${formatTime(etaNext)}` : `Was due ${formatTime(etaNext)}`;
    return (
      <Text size="sm" c="dimmed">
        {dueLine} (no arrival report received)
      </Text>
    );
  }

  if (!etaSource) return null;

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
