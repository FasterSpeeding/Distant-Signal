import { Suspense } from 'react';
import { Alert, Skeleton, Stack, Text, Title } from '@mantine/core';
import { getHistoryRetention } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import {
  availableGranularities,
  granularityShortfallDays,
  resolveGranularity,
  resolveRange,
} from '@/lib/history';
import { GranularityControl } from '@/app/lines/[id]/history/GranularityControl';
import { HistoryRangePicker } from '@/app/lines/[id]/history/HistoryRangePicker';
import { NetworkTrendsResults } from './NetworkTrendsResults';

export const revalidate = 0;

async function resolveRetention(): Promise<{
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}> {
  try {
    const retention = await getHistoryRetention();
    return {
      dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
      halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
    };
  } catch (err) {
    console.warn('Could not resolve retention ceilings; offering only Daily.', err);
    return { dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 };
  }
}

export default async function NetworkHistoryPage({
  searchParams,
}: {
  searchParams: Promise<{ from?: string; to?: string; range?: string; granularity?: string }>;
}) {
  const query = await searchParams;
  const now = Date.now();
  const ceilings = await resolveRetention();
  const range = resolveRange(query, now);
  const rangeWidthMs = Date.parse(range.to) - Date.parse(range.from);
  const available = availableGranularities(rangeWidthMs, ceilings);
  const granularity = resolveGranularity(query, rangeWidthMs, ceilings);
  const granularityShortfall = granularityShortfallDays(range, granularity, ceilings, now);
  const retentionDaysForGranularity =
    granularity === 'day' ? ceilings.dailyStatsRetentionDays : Math.floor(ceilings.halfHourlyStatsRetentionHours / 24);
  const basePath = '/network/history';

  return (
    <Stack p="lg" gap="md">
      <TextLink href="/lines" underline="always">
        Back to all lines
      </TextLink>
      {/* Review M11/§3.5: matches "History: {name}" -- the pattern both
          `/lines/[id]/history` and `/operators/[code]/history` already use
          -- rather than the one-off "Network history" this page shipped
          with. */}
      <Title order={1}>History: Network</Title>
      {/* Review [OH] §3.4/I11: same "say the scope" line the operator
          history page carries under its own title. */}
      <Text c="dimmed" size="sm">
        Every National Rail line this app tracks (TfL not included)
      </Text>
      <HistoryRangePicker basePath={basePath} preset={range.preset} from={range.from} to={range.to} />
      <GranularityControl
        basePath={basePath}
        preset={range.preset}
        from={range.from}
        to={range.to}
        granularity={granularity}
        available={available}
      />
      {granularityShortfall !== null && (
        <Alert color="yellow" variant="light" title="Some of this range isn't available at this granularity">
          This server only keeps {retentionDaysForGranularity}{' '}
          {retentionDaysForGranularity === 1 ? 'day' : 'days'} of data at this granularity. The oldest{' '}
          {granularityShortfall} {granularityShortfall === 1 ? 'day' : 'days'} of the range you picked has
          already been removed — if this range looks empty or short, that may be why, not because nothing
          happened.
        </Alert>
      )}
      <Suspense
        key={`${granularity}-${range.preset ?? `${range.from}-${range.to}`}`}
        fallback={<Skeleton height={320} />}
      >
        <NetworkTrendsResults from={range.from} to={range.to} granularity={granularity} />
      </Suspense>
    </Stack>
  );
}
