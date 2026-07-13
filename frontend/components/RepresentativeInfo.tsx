import { Card, Text } from '@mantine/core';
import type { LineStatus } from '@/lib/types';

/** Shown only when at least one status carries `sampleStats` — the
 * aggregator attaches the same sample-derived stats to every status on a
 * line's report, so the first one found is representative of all of them.
 * Omitted entirely (not zeroed out) when none do. */
export function RepresentativeInfo({ statuses }: { statuses: LineStatus[] }) {
  const withStats = statuses.find((status) => status.sampleStats);
  if (!withStats?.sampleStats) return null;

  const { total, delayed, cancelled, avgDelayMinutes } = withStats.sampleStats;

  return (
    <Card withBorder padding="sm">
      <Text size="sm">
        {delayed} of {total} sampled services delayed, {cancelled} cancelled, avg{' '}
        {avgDelayMinutes.toFixed(1)} min late.
      </Text>
    </Card>
  );
}
