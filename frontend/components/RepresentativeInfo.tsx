import { Card, Text } from '@mantine/core';
import type { LineStatus } from '@/lib/types';

/** Shown only when at least one status carries `sampleStats` — the
 * aggregator attaches the same sample-derived stats to every status on a
 * line's report, so the first one found is representative of all of them.
 * Omitted entirely (not zeroed out) when none do. */
export function RepresentativeInfo({ statuses }: { statuses: LineStatus[] }) {
  const withStats = statuses.find((status) => status.sampleStats);
  if (!withStats?.sampleStats) return null;

  const { total, delayed, cancelled, skipped, avgDelayMinutes } = withStats.sampleStats;
  const cancelledPct = total > 0 ? Math.round((cancelled / total) * 100) : 0;

  return (
    <Card withBorder padding="sm">
      <Text size="sm">
        {delayed} of {total} sampled services delayed, {cancelled} cancelled ({cancelledPct}%), {skipped} skipping
        stops, avg {avgDelayMinutes.toFixed(1)} min late.
      </Text>
    </Card>
  );
}
