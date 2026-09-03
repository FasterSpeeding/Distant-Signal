import { Card, Stack, Text } from '@mantine/core';
import { coverageProvenanceNote } from '@/lib/sampleStats';
import type { LineStatus } from '@/lib/types';

/** Shown only when at least one status carries `sampleStats` or
 * `fullCoverageStats` — the aggregator attaches the same sample-derived (or
 * full-coverage-derived) stats to every status on a line's report, so the
 * first one found is representative of all of them. Omitted entirely (not
 * zeroed out) when neither exists on anything.
 *
 * Decision 1 extends the original sample-only rule: prefer a status
 * carrying `fullCoverageStats`, falling back to one carrying `sampleStats`
 * — the same "prefer the more complete number" precedence
 * `representativeStatus` (`lib/sampleStats.ts`) already applies for
 * summary rows elsewhere. When both are present on the SAME status,
 * `fullCoverageStats` renders (per Decision 1's "prefer full coverage on
 * the numeric columns" table entry) — no line on this app's catalogue
 * carries either today, so this is forward-looking scaffolding, not a
 * behavior change. */
export function RepresentativeInfo({ statuses }: { statuses: LineStatus[] }) {
  const withStats = statuses.find((status) => status.fullCoverageStats) ?? statuses.find((status) => status.sampleStats);
  const stats = withStats?.fullCoverageStats ?? withStats?.sampleStats;
  if (!stats) return null;

  const { total, delayed, cancelled, skipped, avgDelayMinutes } = stats;
  const cancelledPct = total > 0 ? Math.round((cancelled / total) * 100) : 0;
  const provenanceNote = withStats ? coverageProvenanceNote(withStats) : null;

  return (
    <Card withBorder padding="sm">
      <Stack gap={4}>
        <Text size="sm">
          {delayed} of {total} sampled services delayed, {cancelled} cancelled ({cancelledPct}%), {skipped} skipping
          stops, avg {avgDelayMinutes.toFixed(1)} min late.
        </Text>
        {/* Additive trust-signaling, not a second competing number -- see
            Decision 2's "Full coverage, resolved" rendered-copy case. Only
            appears once a status actually carries fullCoverageStats. */}
        {provenanceNote && (
          <Text size="xs" c="dimmed">
            {provenanceNote}
          </Text>
        )}
      </Stack>
    </Card>
  );
}
