import { Suspense } from 'react';
import { Alert, Skeleton, Stack, Title } from '@mantine/core';
import { getAllTocs, getHistoryRetention } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import {
  availableGranularities,
  granularityShortfallDays,
  resolveGranularity,
  resolveRange,
} from '@/lib/history';
import { GranularityControl } from '@/app/lines/[id]/history/GranularityControl';
import { HistoryRangePicker } from '@/app/lines/[id]/history/HistoryRangePicker';
import { OperatorTrendsResults } from './OperatorTrendsResults';

export const revalidate = 0;

/** "TfL" has no `tocs` row (it's a synthetic operator tag, not a real
 * ATOC code -- see spec Open Question 2) -- special-cased the same literal
 * way `AllLinesTable.tsx` already does, since there is no Rust->TypeScript
 * constant bridge to import `common::TFL_OPERATOR` from here. */
async function resolveOperatorName(code: string): Promise<string> {
  if (code === 'TfL') return 'TfL';
  try {
    const tocs = await getAllTocs();
    return tocs.find((toc) => toc.code === code)?.name ?? code;
  } catch (err) {
    console.warn(`Could not resolve a name for operator "${code}"; falling back to the code.`, err);
    return code;
  }
}

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

export default async function OperatorHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ code: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string; granularity?: string }>;
}) {
  const { code } = await params;
  const query = await searchParams;

  const now = Date.now();
  const [name, ceilings] = await Promise.all([resolveOperatorName(code), resolveRetention()]);
  const range = resolveRange(query, now);
  const rangeWidthMs = Date.parse(range.to) - Date.parse(range.from);
  const available = availableGranularities(rangeWidthMs, ceilings);
  const granularity = resolveGranularity(query, rangeWidthMs, ceilings);
  const granularityShortfall = granularityShortfallDays(range, granularity, ceilings, now);
  const retentionDaysForGranularity =
    granularity === 'day' ? ceilings.dailyStatsRetentionDays : Math.floor(ceilings.halfHourlyStatsRetentionHours / 24);
  const basePath = `/operators/${code}/history`;

  return (
    <Stack p="lg" gap="md">
      <TextLink href={`/operators/${code}`} underline="always">
        Back to operator
      </TextLink>
      <Title order={1}>History: {name}</Title>
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
          already been removed -- if this range looks empty or short, that may be why, not because nothing
          happened.
        </Alert>
      )}
      <Suspense
        key={`${granularity}-${range.preset ?? `${range.from}-${range.to}`}`}
        fallback={<Skeleton height={320} />}
      >
        <OperatorTrendsResults code={code} from={range.from} to={range.to} granularity={granularity} />
      </Suspense>
    </Stack>
  );
}
