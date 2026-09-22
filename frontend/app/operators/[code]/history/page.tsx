import { Suspense } from 'react';
import { Alert, Skeleton, Stack, Text, Title } from '@mantine/core';
import { getAllTocs, getHistoryRetention, getOperator } from '@/lib/api';
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
 * ATOC code -- see spec Open Question 2), so it needs an explicit
 * early-return here rather than a `tocs`-lookup-with-fallback: there is no
 * Rust->TypeScript constant bridge to import `common::TFL_OPERATOR` from
 * this file. `AllLinesTable.tsx` reaches the same displayed value a
 * different way -- it has no literal `code === 'TfL'` branch at all, just a
 * `nameByCode` map built from `tocs` with a fallback to the raw code when a
 * code has no map entry, which happens to read as "TfL" only because the
 * code and the desired display string are the same string. */
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

/** How many lines this operator's rollup currently covers, for the "N
 * lines" scope line under the `<h1>` (review [OH] §3.4/I11: "History:
 * London North Eastern Railway" gives no sense of scope until the last
 * sentence of the methodology paragraph). A second, independent fetch from
 * `resolveOperatorName`'s -- rather than folding the two together -- so a
 * failure here (network hiccup, or a real `tocs` code that currently
 * matches zero lines, `ApiNotFoundError`) only drops this one line rather
 * than risking the page's own `<h1>` name resolution, which already has
 * its own tested fallback shape. `null` means "don't know" and the caller
 * renders nothing rather than a wrong or misleading count. */
async function resolveLineCount(code: string): Promise<number | null> {
  try {
    const operator = await getOperator(code);
    return operator.lineIds.length;
  } catch (err) {
    console.warn(`Could not resolve a line count for operator "${code}".`, err);
    return null;
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
  const [name, lineCount, ceilings] = await Promise.all([
    resolveOperatorName(code),
    resolveLineCount(code),
    resolveRetention(),
  ]);
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
      <TextLink href="/operators" underline="always">
        Back to operators
      </TextLink>
      <Title order={1}>History: {name}</Title>
      {/* Review [OH] §3.4/I11: says what this rollup covers right under the
          title, rather than leaving scope to be inferred from the last
          sentence of the methodology paragraph further down the page. */}
      {lineCount !== null && (
        <Text c="dimmed" size="sm">
          {lineCount} {lineCount === 1 ? 'line' : 'lines'}
        </Text>
      )}
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
        <OperatorTrendsResults code={code} from={range.from} to={range.to} granularity={granularity} />
      </Suspense>
    </Stack>
  );
}
