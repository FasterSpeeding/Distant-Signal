'use client';

import { useMemo, useState } from 'react';
import {
  Stack,
  Table,
  TableThead,
  TableTbody,
  TableTr,
  TableTh,
  TableTd,
  Text,
  MultiSelect,
} from '@mantine/core';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { worstStatus, severityRank } from '@/lib/severity';
import type { LineStatusReport, LineSummary } from '@/lib/types';

function sampleStatsFor(report: LineStatusReport | undefined) {
  return report?.lineStatuses.find((status) => status.sampleStats)?.sampleStats;
}

type SortField = 'name' | 'status' | 'avgDelay' | 'cancelled';
type SortState = { field: SortField; direction: 'asc' | 'desc' };

function sortIndicator(field: SortField, sort: SortState | null) {
  if (!sort || sort.field !== field) return null;
  return sort.direction === 'asc' ? ' ▲' : ' ▼';
}

export function AllLinesTable({
  lines,
  reports,
  pinnedLineIds,
}: {
  lines: LineSummary[];
  reports: LineStatusReport[];
  pinnedLineIds: string[];
}) {
  const [selectedOperators, setSelectedOperators] = useState<string[]>([]);
  const [sort, setSort] = useState<SortState | null>(null);

  const reportsById = useMemo(() => new Map(reports.map((report) => [report.id, report])), [reports]);
  const pinnedSet = useMemo(() => new Set(pinnedLineIds), [pinnedLineIds]);

  const operatorOptions = useMemo(
    () => Array.from(new Set(lines.flatMap((line) => line.operators))).sort(),
    [lines],
  );

  const rows = useMemo(
    () =>
      lines.map((line) => {
        const report = reportsById.get(line.id);
        const worst = report ? worstStatus(report) : undefined;
        const stats = sampleStatsFor(report);
        const cancelledPct = stats && stats.total > 0 ? Math.round((stats.cancelled / stats.total) * 100) : null;
        return { line, worst, stats, cancelledPct };
      }),
    [lines, reportsById],
  );

  const filteredRows = useMemo(() => {
    if (selectedOperators.length === 0) return rows;
    return rows.filter((row) => row.line.operators.some((op) => selectedOperators.includes(op)));
  }, [rows, selectedOperators]);

  // Missing values (no report, no sample stats) always sort to the end,
  // regardless of direction -- flipping direction shouldn't make "unknown"
  // rows jump to the top.
  const sortedRows = useMemo(() => {
    if (!sort) return filteredRows;
    const { field, direction } = sort;
    const sign = direction === 'asc' ? 1 : -1;

    function rankFor(row: (typeof filteredRows)[number]): number | null {
      switch (field) {
        case 'status':
          return row.worst ? severityRank(row.worst.statusSeverity) : null;
        case 'avgDelay':
          return row.stats ? row.stats.avgDelayMinutes : null;
        case 'cancelled':
          return row.cancelledPct;
        default:
          return null;
      }
    }

    return [...filteredRows].sort((a, b) => {
      if (field === 'name') {
        return sign * a.line.name.localeCompare(b.line.name);
      }
      const av = rankFor(a);
      const bv = rankFor(b);
      if (av === null && bv === null) return 0;
      if (av === null) return 1;
      if (bv === null) return -1;
      return sign * (av - bv);
    });
  }, [filteredRows, sort]);

  function toggleSort(field: SortField) {
    setSort((prev) => {
      if (!prev || prev.field !== field) return { field, direction: 'asc' };
      return { field, direction: prev.direction === 'asc' ? 'desc' : 'asc' };
    });
  }

  function headerProps(field: SortField) {
    return {
      onClick: () => toggleSort(field),
      style: { cursor: 'pointer', userSelect: 'none' as const },
    };
  }

  return (
    <Stack gap="md">
      <MultiSelect
        label="Filter by operator"
        placeholder="All operators"
        data={operatorOptions}
        value={selectedOperators}
        onChange={setSelectedOperators}
        clearable
        clearButtonProps={{ 'aria-label': 'Clear operator filter' }}
      />
      <Table>
        {/* Flat `TableThead`/`TableTr`/... named exports, not the
            `Table.Thead` dot-notation compound API -- kept consistent with
            the rest of this codebase's Table usage even though this is a
            Client Component (see the Server Component variant this was
            extracted from for why the flat exports matter there). */}
        <TableThead>
          <TableTr>
            <TableTh {...headerProps('name')}>Name{sortIndicator('name', sort)}</TableTh>
            <TableTh {...headerProps('status')}>Status{sortIndicator('status', sort)}</TableTh>
            <TableTh {...headerProps('avgDelay')}>Avg Delay{sortIndicator('avgDelay', sort)}</TableTh>
            <TableTh {...headerProps('cancelled')}>Cancelled{sortIndicator('cancelled', sort)}</TableTh>
            <TableTh>Pin</TableTh>
          </TableTr>
        </TableThead>
        <TableTbody>
          {sortedRows.map(({ line, worst, stats, cancelledPct }) => (
            <TableTr key={line.id}>
              <TableTd>
                <TextLink href={`/lines/${line.id}`}>{line.name}</TextLink>
              </TableTd>
              <TableTd>{worst ? <StatusBadge severity={worst.statusSeverity} /> : null}</TableTd>
              <TableTd>
                {stats ? (
                  <Text size="sm">{stats.avgDelayMinutes.toFixed(1)} min</Text>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )}
              </TableTd>
              <TableTd>
                {cancelledPct !== null ? (
                  <Text size="sm">{cancelledPct}%</Text>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )}
              </TableTd>
              <TableTd>
                <PinToggle kind="line" id={line.id} initiallyPinned={pinnedSet.has(line.id)} />
              </TableTd>
            </TableTr>
          ))}
        </TableTbody>
      </Table>
    </Stack>
  );
}
