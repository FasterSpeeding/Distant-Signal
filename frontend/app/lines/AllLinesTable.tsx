'use client';

import { useId, useMemo, useState } from 'react';
import {
  Badge,
  Chip,
  ChipGroup,
  Group,
  Stack,
  Table,
  TableThead,
  TableTbody,
  TableTr,
  TableTh,
  TableTd,
  Text,
  TextInput,
  MultiSelect,
  UnstyledButton,
  Tooltip,
} from '@mantine/core';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { worstStatus, severityRank } from '@/lib/severity';
import { cancelledPercent, formatSampleSummary, representativeStatus, sampleUnavailableReason } from '@/lib/sampleStats';
import { countryForReport, type Country } from '@/lib/modes';
import type { LineStatus, LineStatusReport, LineSummary, Suggestion } from '@/lib/types';

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

/** Task 3.4.1: sort-by-status gets a defined rank for "no data" rows,
 * rather than falling into the generic "missing value always sorts last"
 * handling `rankFor` gives the two numeric columns -- a rank participates
 * in ascending/descending order like any real severity, instead of being
 * pinned to one end regardless of direction. Placed below every real
 * severity (`severityRank`'s range is 0-4): "no data" is not a claim about
 * how good or bad service is, but ranking it below "good" keeps ascending
 * order reading as "best-known state first", worst last, missing last of
 * all -- the most natural fit for a table whose primary job is surfacing
 * problems. */
const NO_DATA_STATUS_RANK = -1;

/** The "NO DATA" badge's tooltip reason (Task 3.4.1). Mirrors the numeric
 * columns' own `representative ? sampleUnavailableReason(representative) :
 * <plain dash>` shape immediately below for the same reason -- `worst` and
 * `representative` are both derived from the same possibly-absent
 * `report` (see `rows` above), so today `representative` is always
 * `undefined` whenever `worst` is. Kept conditional anyway so this stays
 * correct if that ever changes, rather than silently going stale. */
function noStatusReason(representative: LineStatus | undefined): string {
  if (representative) {
    return sampleUnavailableReason(representative) ?? 'No status has been computed for this line yet.';
  }
  return 'No status has been computed for this line yet.';
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

const COUNTRY_LABELS: Record<Country, string> = {
  Gb: 'GB',
  NorthernIreland: 'Northern Ireland',
  RepublicOfIreland: 'Republic of Ireland',
};

/** Mirrors `chipRowLabel` in `components/IssueList.tsx:131-133` --
 * duplicated rather than imported since that function isn't exported and
 * this is a one-line, component-local concern in both places (same as
 * `expandOperatorForFiltering` being its own component-local helper
 * rather than shared). States explicitly that an empty selection filters
 * nothing, not "filters to nothing" -- the exact confusion that file's own
 * comment names. */
function countryChipLabel(selected: number): string {
  return selected === 0 ? 'Country — showing all' : `Country — ${selected} selected`;
}

export function AllLinesTable({
  lines,
  reports,
  pinnedLineIds,
  tocs,
  viewerIsAnonymous = false,
}: {
  lines: LineSummary[];
  reports: LineStatusReport[];
  pinnedLineIds: string[];
  tocs: Suggestion[];
  // Task 3.4.13: hints the pin star that pinning needs an account, for a
  // visitor who has none yet. Optional, defaulting to `false`, so every
  // existing caller/test that doesn't pass it keeps today's behaviour.
  viewerIsAnonymous?: boolean;
}) {
  const [selectedOperators, setSelectedOperators] = useState<string[]>([]);
  const [selectedCountries, setSelectedCountries] = useState<Country[]>([]);
  const [nameQuery, setNameQuery] = useState('');
  // Task 3.4.2: defaults to name order rather than the catalogue's own
  // (arbitrary-looking) ordering, so a first-time visitor to a 125-row
  // table lands somewhere navigable instead of needing to discover sorting
  // exists first.
  const [sort, setSort] = useState<SortState | null>({ field: 'name', direction: 'asc' });
  const countryLabelId = useId();

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
        const representative = representativeStatus(report?.lineStatuses ?? []);
        // Decision 1: prefer full-coverage numbers over sample numbers when
        // both exist -- derived from the SAME `representative` status used
        // for the subtitle/tooltip below, not a separate scan, so "which
        // status is representative" and "which numbers render" never
        // disagree.
        const stats = representative?.fullCoverageStats ?? representative?.sampleStats;
        const cancelledPct = cancelledPercent(stats);
        // A line with no report at all has no modeName to derive a country
        // from; every such line today is GB by construction (see
        // docs/superpowers/plans/2026-09-05-country-filtering-plan.md
        // Judgment Call 3 -- no non-GB LineSummary is reachable through
        // GET /public/lines yet), so it defaults the same way
        // countryForReport itself defaults an unrecognised modeName.
        const country: Country = report ? countryForReport(report) : 'Gb';
        return { line, worst, stats, cancelledPct, representative, country };
      }),
    [lines, reportsById],
  );

  // Mirrors operatorOptions's own "derive the option set from what's
  // actually present" pattern above, over `rows` rather than raw `lines`
  // since country is only knowable once a line is joined to its report.
  // This is also this feature's self-hiding gate (Decision 4/5): a length
  // of 1 (today, always exactly ['Gb']) means the filter control below does
  // not render at all.
  const countryOptions = useMemo(() => Array.from(new Set(rows.map((row) => row.country))).sort(), [rows]);

  const filteredRows = useMemo(() => {
    let result = rows;
    // Task 3.4.2: a plain case-insensitive substring match against the
    // line's own name -- unlike the operator filter's server-sourced
    // `Suggestion` list, every line is already loaded client-side here, so
    // there is nothing to fetch suggestions for. AND-combined with the
    // other two filters below, same posture as country/operator.
    const trimmedQuery = nameQuery.trim().toLowerCase();
    if (trimmedQuery.length > 0) {
      result = result.filter((row) => row.line.name.toLowerCase().includes(trimmedQuery));
    }
    if (selectedOperators.length > 0) {
      // Expand the selection (e.g. "TfL" -> "TfL"/"LO"/"XR"), not each row's
      // own `operators` -- the option list and a line's own displayed code
      // must stay exact, only what a selection matches against widens.
      const expandedSelection = new Set(selectedOperators.flatMap(expandOperatorForFiltering));
      result = result.filter((row) => row.line.operators.some((op) => expandedSelection.has(op)));
    }
    // AND-combined with the operator filter, not folded into it (Decision
    // 4): operator and country answer different questions, and a line has
    // exactly one country but potentially several operators.
    if (selectedCountries.length > 0) {
      result = result.filter((row) => selectedCountries.includes(row.country));
    }
    return result;
  }, [rows, selectedOperators, selectedCountries, nameQuery]);

  // Missing avgDelay/cancelled values (no sample stats) always sort to the
  // end, regardless of direction -- flipping direction shouldn't make
  // "unknown" rows jump to the top. Status is the one exception: it has its
  // own defined `NO_DATA_STATUS_RANK` bucket instead (Task 3.4.1), so it
  // sorts like any other rank rather than through this null-to-the-end path.
  const sortedRows = useMemo(() => {
    if (!sort) return filteredRows;
    const { field, direction } = sort;
    const sign = direction === 'asc' ? 1 : -1;

    function rankFor(row: (typeof filteredRows)[number]): number | null {
      switch (field) {
        case 'status':
          return row.worst ? severityRank(row.worst.statusSeverity) : NO_DATA_STATUS_RANK;
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
      {/* Task 3.4.2: `linesFilterBar` (app/globals.css) makes this whole
          block sticky below the `sm` breakpoint only -- 125 rows is a long
          scroll on a phone, and losing the filters/search off the top of
          the screen meant scrolling all the way back up to change either.
          Left as ordinary flow above `sm`, where the table is short enough
          relative to the viewport that stickiness buys nothing. */}
      <Stack gap="md" className="linesFilterBar">
        <Group gap="md" align="flex-start" wrap="wrap">
          <TextInput
            label="Find a line"
            placeholder="e.g. West Coast Main Line"
            value={nameQuery}
            onChange={(event) => setNameQuery(event.currentTarget.value)}
            style={{ flex: '1 1 220px' }}
          />
          <MultiSelect
            label="Filter by operator"
            placeholder="All operators"
            data={operatorOptions}
            value={selectedOperators}
            onChange={setSelectedOperators}
            searchable
            clearable
            clearButtonProps={{ 'aria-label': 'Clear operator filter' }}
            style={{ flex: '1 1 220px' }}
          />
        </Group>
        {/* Self-hiding per Decision 4/5: with fewer than two countries present
            (today, always exactly ['Gb']) there is nothing meaningful to
            filter by, and a one-option control is worse than no control at
            all -- see
            docs/superpowers/specs/2026-09-05-country-filtering-design.md §5. */}
        {countryOptions.length > 1 && (
          <Stack gap={4}>
            <Text id={countryLabelId} size="xs" fw={600} c="dimmed">
              {countryChipLabel(selectedCountries.length)}
            </Text>
            <ChipGroup multiple value={selectedCountries} onChange={(value) => setSelectedCountries(value as Country[])}>
              <Group gap="xs" role="group" aria-labelledby={countryLabelId}>
                {countryOptions.map((country) => (
                  <Chip
                    key={country}
                    value={country}
                    size="xs"
                    variant={selectedCountries.includes(country) ? 'filled' : 'outline'}
                  >
                    {COUNTRY_LABELS[country]}
                  </Chip>
                ))}
              </Group>
            </ChipGroup>
          </Stack>
        )}
      </Stack>
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
            {/* Task 3.4.7: `white-space: nowrap` so "Avg Delay ↕" (the
                longest of the four labels) can't wrap its own sort glyph
                onto a second line, orphaning it away from the label it
                belongs to. */}
            <TableTh aria-sort={ariaSort('name', sort)} style={{ whiteSpace: 'nowrap' }}>
              <UnstyledButton onClick={() => toggleSort('name')} style={{ fontWeight: 'inherit' }}>
                Name
                <SortGlyph field="name" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh aria-sort={ariaSort('status', sort)} style={{ whiteSpace: 'nowrap' }}>
              <UnstyledButton onClick={() => toggleSort('status')} style={{ fontWeight: 'inherit' }}>
                Status
                <SortGlyph field="status" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh aria-sort={ariaSort('avgDelay', sort)} visibleFrom="sm" style={{ whiteSpace: 'nowrap' }}>
              <UnstyledButton onClick={() => toggleSort('avgDelay')} style={{ fontWeight: 'inherit' }}>
                Avg Delay
                <SortGlyph field="avgDelay" sort={sort} />
              </UnstyledButton>
            </TableTh>
            <TableTh aria-sort={ariaSort('cancelled', sort)} visibleFrom="sm" style={{ whiteSpace: 'nowrap' }}>
              <UnstyledButton onClick={() => toggleSort('cancelled')} style={{ fontWeight: 'inherit' }}>
                Cancelled
                <SortGlyph field="cancelled" sort={sort} />
              </UnstyledButton>
            </TableTh>
            {/* No `visibleFrom="sm"`, unlike the two numeric columns beside it.
                Those are hidden on mobile only because they are re-surfaced in the
                `hiddenFrom="sm"` sub-line under the line name (:225) -- Pin got the
                hiding half of that pattern without the re-surfacing half in
                bd4d739, and unlike a number it is an interactive control with no
                other home: `PinToggle` exists in exactly two places in this app,
                here and on the station detail page (`kind="station"`), so below the
                sm breakpoint there was no way to pin or unpin a LINE anywhere in
                the application, and a pinned row was visually identical to an
                unpinned one. See
                docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F4. */}
            <TableTh>Pin</TableTh>
          </TableTr>
        </TableThead>
        <TableTbody>
          {sortedRows.map(({ line, worst, stats, cancelledPct, representative }) => (
            <TableTr key={line.id}>
              <TableTd>
                {/* Task 3.4.8: at narrow widths a wrapped two-line name read
                    as two separate stacked list items rather than one
                    wrapped link, at Mantine `Text`'s default (looser)
                    line-height. */}
                <TextLink href={`/lines/${line.id}`} lh={1.3}>
                  {line.name}
                </TextLink>
                {/* At 390px five columns cannot all fit, and the one that
                    was losing was Status — the page's whole point — while
                    two numeric columns kept their width. Below `sm` the
                    numbers move here instead of disappearing; `visibleFrom`/
                    `hiddenFrom` are Mantine's `display: none` classes,
                    emitted by MantineProvider on server and client alike,
                    so this is SSR-safe (unlike `useMediaQuery`). */}
                <Text size="xs" c="dimmed" hiddenFrom="sm">
                  {formatSampleSummary(representative)}
                </Text>
              </TableTd>
              <TableTd>
                {worst ? (
                  <StatusBadge severity={worst.statusSeverity} />
                ) : (
                  // Task 3.4.1: previously rendered nothing at all -- ~120
                  // of ~125 rows had no `line_status` row yet (see `rows`
                  // above), so the Status column was blank for almost every
                  // line, with no way to tell "good service" from "no data"
                  // from "not computed yet". Grey/outline, not one of
                  // StatusBadge's severity colours (a stated Non-goal is
                  // leaving `severity.ts`'s hue map untouched) -- this is
                  // the absence of a status, not one more severity.
                  // `data-status-badge` opts out of Mantine Badge's default
                  // ellipsis truncation, the same fix `/lines/[id]`'s own
                  // "No status yet" badge and `StatusBadge` itself already
                  // rely on (see app/globals.css's `[data-status-badge]`
                  // rule) -- otherwise "NO DATA" clips to "N…" in this same
                  // table at narrow widths.
                  <Tooltip label={noStatusReason(representative)}>
                    <Badge color="gray" variant="outline" data-status-badge>
                      NO DATA
                    </Badge>
                  </Tooltip>
                )}
              </TableTd>
              <TableTd visibleFrom="sm">
                {stats ? (
                  <Text size="sm">{stats.avgDelayMinutes.toFixed(1)} min</Text>
                ) : representative ? (
                  <Tooltip label={sampleUnavailableReason(representative)}>
                    <Text size="sm" c="dimmed">
                      —
                    </Text>
                  </Tooltip>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )}
              </TableTd>
              <TableTd visibleFrom="sm">
                {cancelledPct !== null ? (
                  <Text size="sm">{cancelledPct}%</Text>
                ) : representative ? (
                  <Tooltip label={sampleUnavailableReason(representative)}>
                    <Text size="sm" c="dimmed">
                      —
                    </Text>
                  </Tooltip>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )}
              </TableTd>
              <TableTd>
                <PinToggle
                  kind="line"
                  id={line.id}
                  initiallyPinned={pinnedSet.has(line.id)}
                  needsAccountHint={viewerIsAnonymous}
                />
              </TableTd>
            </TableTr>
          ))}
        </TableTbody>
      </Table>
    </Stack>
  );
}
