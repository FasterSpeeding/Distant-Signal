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
  UnstyledButton,
} from '@mantine/core';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { worstStatus, severityRank } from '@/lib/severity';
import { firstSampleStats, cancelledPercent, formatSampleSummary } from '@/lib/sampleStats';
import type { LineStatusReport, LineSummary, Suggestion } from '@/lib/types';

type SortField = 'name' | 'status' | 'avgDelay' | 'cancelled';
type SortState = { field: SortField; direction: 'asc' | 'desc' };

/** A neutral glyph on every sortable column, not just the active one:
 * without it there was no affordance at all until after a click, so the
 * headers looked like plain labels. `aria-hidden` because `aria-sort` on
 * the `<th>` carries the same information properly. */
function SortGlyph({ field, sort }: { field: SortField; sort: SortState | null }) {
  const active = sort?.field === field;
  return (
    <Text span size="xs" c="dimmed" aria-hidden>
      {' '}
      {active ? (sort!.direction === 'asc' ? '▲' : '▼') : '↕'}
    </Text>
  );
}

function ariaSort(field: SortField, sort: SortState | null): 'ascending' | 'descending' | 'none' {
  if (sort?.field !== field) return 'none';
  return sort.direction === 'asc' ? 'ascending' : 'descending';
}

/** Real NR-side operator codes for TfL-branded railways that a `line`'s own
 * `operators` array tags with their own code rather than `"TfL"` -- London
 * Overground is `"LO"`, the Elizabeth line is `"XR"` (see
 * `TFL_TO_NR_LINE_ID` in `crates/common/src/lib.rs`). To a passenger both
 * are TfL services, so filtering by "TfL" should surface them too even
 * though nothing in the data literally says "TfL" on those rows. */
const TFL_ADJACENT_OPERATORS = ['LO', 'XR'];

/** Widens one selected operator filter value into every code it should
 * match against a row's `operators` array. One-directional: selecting
 * "TfL" also matches "LO"/"XR" rows, but selecting "LO" or "XR" directly
 * still matches only that code -- Overground/Elizabeth line riders can
 * filter to just their own line without pulling in the Tube. This is
 * filter-matching only; it must not change what `operatorOptions` lists or
 * what code a line's own badge displays. */
export function expandOperatorForFiltering(operator: string): string[] {
  return operator === 'TfL' ? [operator, ...TFL_ADJACENT_OPERATORS] : [operator];
}

export function AllLinesTable({
  lines,
  reports,
  pinnedLineIds,
  tocs,
}: {
  lines: LineSummary[];
  reports: LineStatusReport[];
  pinnedLineIds: string[];
  tocs: Suggestion[];
}) {
  const [selectedOperators, setSelectedOperators] = useState<string[]>([]);
  const [sort, setSort] = useState<SortState | null>(null);

  const reportsById = useMemo(() => new Map(reports.map((report) => [report.id, report])), [reports]);
  const pinnedSet = useMemo(() => new Set(pinnedLineIds), [pinnedLineIds]);
  const nameByCode = useMemo(() => new Map(tocs.map((toc) => [toc.code, toc.name])), [tocs]);

  // `label` carries both the code and the name (not just the code, with
  // the name relegated to a separate renderOption) so Mantine's default
  // label-based dropdown filter can match a search on either -- the
  // station-search autocomplete elsewhere in this app had a bug from
  // getting this backwards.
  const operatorOptions = useMemo(
    () =>
      Array.from(new Set(lines.flatMap((line) => line.operators)))
        .sort()
        .map((code) => {
          const name = nameByCode.get(code);
          return { value: code, label: name ? `${code} - ${name}` : code };
        }),
    [lines, nameByCode],
  );

  const rows = useMemo(
    () =>
      lines.map((line) => {
        const report = reportsById.get(line.id);
        const worst = report ? worstStatus(report) : undefined;
        const stats = firstSampleStats(report?.lineStatuses ?? []);
        const cancelledPct = cancelledPercent(stats);
        return { line, worst, stats, cancelledPct };
      }),
    [lines, reportsById],
  );

  const filteredRows = useMemo(() => {
    if (selectedOperators.length === 0) return rows;
    // Expand the selection (e.g. "TfL" -> "TfL"/"LO"/"XR"), not each row's
    // own `operators` -- the option list and a line's own displayed code
    // must stay exact, only what a selection matches against widens.
    const expandedSelection = new Set(selectedOperators.flatMap(expandOperatorForFiltering));
    return rows.filter((row) => row.line.operators.some((op) => expandedSelection.has(op)));
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

  return (
    <Stack gap="md">
      <MultiSelect
        label="Filter by operator"
        placeholder="All operators"
        data={operatorOptions}
        value={selectedOperators}
        onChange={setSelectedOperators}
        searchable
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
            {/* `UnstyledButton` inside the `<th>` rather than `onClick` on
                the `<th>` itself: a bare cell with a click handler is not
                focusable and cannot be triggered from the keyboard, which
                made the whole sorting feature mouse-only. */}
            <TableTh aria-sort={ariaSort('name', sort)}>
              <UnstyledButton onClick={() => toggleSort('name')} style={{ fontWeight: 'inherit' }}>
                Name
                <SortGlyph field="name" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh aria-sort={ariaSort('status', sort)}>
              <UnstyledButton onClick={() => toggleSort('status')} style={{ fontWeight: 'inherit' }}>
                Status
                <SortGlyph field="status" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh aria-sort={ariaSort('avgDelay', sort)} visibleFrom="sm">
              <UnstyledButton onClick={() => toggleSort('avgDelay')} style={{ fontWeight: 'inherit' }}>
                Avg Delay
                <SortGlyph field="avgDelay" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh aria-sort={ariaSort('cancelled', sort)} visibleFrom="sm">
              <UnstyledButton onClick={() => toggleSort('cancelled')} style={{ fontWeight: 'inherit' }}>
                Cancelled
                <SortGlyph field="cancelled" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh>Pin</TableTh>
          </TableTr>
        </TableThead>
        <TableTbody>
          {sortedRows.map(({ line, worst, stats, cancelledPct }) => (
            <TableTr key={line.id}>
              <TableTd>
                <TextLink href={`/lines/${line.id}`}>{line.name}</TextLink>
                {/* At 390px five columns cannot all fit, and the one that
                    was losing was Status — the page's whole point — while
                    two numeric columns kept their width. Below `sm` the
                    numbers move here instead of disappearing; `visibleFrom`/
                    `hiddenFrom` are Mantine's `display: none` classes,
                    emitted by MantineProvider on server and client alike,
                    so this is SSR-safe (unlike `useMediaQuery`). */}
                <Text size="xs" c="dimmed" hiddenFrom="sm">
                  {formatSampleSummary(stats)}
                </Text>
              </TableTd>
              <TableTd>{worst ? <StatusBadge severity={worst.statusSeverity} /> : null}</TableTd>
              <TableTd visibleFrom="sm">
                {stats ? (
                  <Text size="sm">{stats.avgDelayMinutes.toFixed(1)} min</Text>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )}
              </TableTd>
              <TableTd visibleFrom="sm">
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
