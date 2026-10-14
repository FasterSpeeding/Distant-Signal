'use client';

import { Fragment, useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  Box,
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
  MultiSelect,
  UnstyledButton,
  Tooltip,
  VisuallyHidden,
} from '@mantine/core';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import {
  worstStatus,
  severityRank,
  severityGroup,
  SEVERITY_GROUP_LABELS,
  severityGroupLabel,
  SEVERITY_GROUPS_BY_RANK,
  type SeverityGroup,
} from '@/lib/severity';
import {
  cancelledPercent,
  formatSampleSummary,
  representativeStatus,
  sampleUnavailableReason,
} from '@/lib/sampleStats';
import { countryForReport, type Country } from '@/lib/modes';
import type { LineStatus, LineStatusReport, LineSummary, Suggestion } from '@/lib/types';
import { fullTitle } from '@/lib/pageMetadata';
import { LinesFilterBar } from './LinesFilterBar';
import { StateBadge } from '@/components/StateBadge';
import { noOptionsFound } from '@/components/noOptionsFound';

type SortField = 'name' | 'status' | 'avgDelay' | 'cancelled';
interface SortState {
  field: SortField;
  direction: 'asc' | 'desc';
}

/** Feather-style chevrons: up, down, or both stacked for an unsorted
 * column. 12px, `currentColor`, decorative. */
function SortChevron({ direction }: { direction: 'asc' | 'desc' | null }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      data-sort-chevron={direction ?? 'none'}
    >
      {direction !== 'desc' && <polyline points={direction === 'asc' ? '6 15 12 9 18 15' : '7 10 12 5 17 10'} />}
      {direction !== 'asc' && <polyline points={direction === 'desc' ? '6 9 12 15 18 9' : '7 14 12 19 17 14'} />}
    </svg>
  );
}

/** A neutral glyph on every sortable column, not just the active one:
 * without it there was no affordance at all until after a click, so the
 * headers looked like plain labels. `aria-hidden` because `aria-sort` on
 * the `<th>` carries the same information properly. */
function SortGlyph({ field, sort }: { field: SortField; sort: SortState | null }) {
  const active = sort?.field === field;
  return (
    <Text span size="xs" c="dimmed" aria-hidden className="sortHeader__glyph">
      <SortChevron direction={active ? sort.direction : null} />
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

/** The "No data" badge's tooltip reason (Task 3.4.1). Mirrors the numeric
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

/** An empty numeric cell (no sampled rate): an en dash keeps the column
 * scannable at a glance; a screen reader hears "No data" instead.
 * docs/style-guide.md, Writing: placeholders. */
const NO_VALUE = (
  <Text size="sm" c="dimmed">
    <span aria-hidden="true">–</span>
    <VisuallyHidden>No data</VisuallyHidden>
  </Text>
);

/** Mirrors `chipRowLabel` in `components/IssueList.tsx:131-133` --
 * duplicated rather than imported since that function isn't exported and
 * this is a one-line, component-local concern in both places (same as
 * `expandOperatorForFiltering` being its own component-local helper
 * rather than shared). States explicitly that an empty selection filters
 * nothing, not "filters to nothing" -- the exact confusion that file's own
 * comment names. */
function countryChipLabel(selected: number): string {
  return selected === 0 ? 'Country: showing all' : `Country: ${selected} selected`;
}

/** Mirrors `countryChipLabel` immediately above -- single-select, not
 * multi (a line has exactly one worst status right now, so "2 selected"
 * would never be a meaningful state the way it is for operator/country). */
function statusChipLabel(selected: SeverityGroup | null): string {
  return selected ? `Status: ${groupChipLabel(selected)}` : 'Status: showing all';
}

/** A severity group's name as a chip or label, in sentence case ("Minor
 * disruption"), matching the sentence-case status badges in the table
 * below and the /status tiles (`severityGroupLabel`). */
const groupChipLabel = severityGroupLabel;

/** The query params this table keeps in the URL. `statusGroup` is also
 * what `app/status/page.tsx`'s counter tiles link to. */
export const LINES_QUERY_PARAM = 'q';
export const LINES_OPERATOR_PARAM = 'operator';
export const LINES_STATUS_GROUP_PARAM = 'statusGroup';

/** How long the URL waits for typing to settle before it is rewritten.
 * Safari throttles `history.replaceState` (and throws past the limit), so
 * one call per keystroke is not safe. */
const URL_SYNC_DELAY_MS = 300;

/** The current URL with the filter params replaced by `state`, leaving any
 * other params alone. */
export function linesFilterUrl(
  current: string,
  state: { query: string; operators: readonly string[]; statusGroup: SeverityGroup | null },
): string {
  const url = new URL(current);
  const params = url.searchParams;
  params.delete(LINES_QUERY_PARAM);
  params.delete(LINES_OPERATOR_PARAM);
  params.delete(LINES_STATUS_GROUP_PARAM);
  const query = state.query.trim();
  if (query) params.set(LINES_QUERY_PARAM, query);
  for (const operator of state.operators) params.append(LINES_OPERATOR_PARAM, operator);
  if (state.statusGroup) params.set(LINES_STATUS_GROUP_PARAM, state.statusGroup);
  const search = params.toString();
  return `${url.pathname}${search ? `?${search}` : ''}${url.hash}`;
}

/** A line name with a line-break opportunity after every "/". Names like
 * "West Wales Line (Swansea/Carmarthen - Milford Haven/Fishguard/Pembroke
 * Dock)" have no spaces around the slashes, so "Haven/Fishguard/Pembroke"
 * was one 212px unbreakable word that set the Name column's minimum width
 * and pushed the table past a 390px screen. */
export function breakableLineName(name: string): ReactNode {
  const parts = name.split('/');
  if (parts.length === 1) return name;
  return parts.map((part, index) => (
    <Fragment key={index}>
      {index > 0 && '/'}
      {index > 0 && <wbr />}
      {part}
    </Fragment>
  ));
}

/** The browser tab title for the current filters, or `null` when nothing
 * is filtered. Status first (what the dashboard tiles link to), then
 * operators by name (style guide: operators appear by name, never code),
 * then the search text in quotes. */
export function filteredTitle(
  statusGroup: SeverityGroup | null,
  operatorNames: readonly string[],
  query: string,
): string | null {
  const parts: string[] = [];
  if (statusGroup) parts.push(SEVERITY_GROUP_LABELS[statusGroup]);
  const [onlyOperator] = operatorNames;
  if (operatorNames.length > 1) parts.push(`${operatorNames.length} operators`);
  else if (onlyOperator) parts.push(onlyOperator);
  const trimmed = query.trim();
  if (trimmed) parts.push(`"${trimmed}"`);
  return parts.length > 0 ? `Lines: ${parts.join(', ')}` : null;
}

export function AllLinesTable({
  lines,
  reports,
  pinnedLineIds,
  tocs,
  viewerIsAnonymous = false,
  initialStatusGroup,
  initialOperators,
  initialQuery,
}: {
  lines: LineSummary[];
  reports: LineStatusReport[];
  pinnedLineIds: string[];
  tocs: Suggestion[];
  // Task 3.4.13: hints the pin star that pinning needs an account, for a
  // visitor who has none yet. Optional, defaulting to `false`, so every
  // existing caller/test that doesn't pass it keeps today's behaviour.
  viewerIsAnonymous?: boolean;
  /** Seeds the new status-group filter below from a query param (e.g.
   * `/lines?statusGroup=severe`, followed from `app/status/page.tsx`'s
   * counter tiles) -- consumed only as this `useState`'s initial value on
   * mount, same as `IncidentSearchForm`'s own `initialOperator`/
   * `initialLine`/`initialFrom`/`initialTo` props. Unlike those, this one
   * IS written back to afterwards -- see the URL sync effect
   * below (2026-09-22 UX review §3.2: the chip used to only ever seed
   * state, never update the URL, so tapping a dashboard tile then
   * clearing the filter left `?statusGroup=severe` in the address bar
   * disagreeing with an unfiltered table). */
  initialStatusGroup?: SeverityGroup | undefined;
  /** Seeds the operator filter from `?operator=` (repeatable). Codes no
   * line carries are dropped. */
  initialOperators?: readonly string[] | undefined;
  /** Seeds "Find a line" from `?q=`. */
  initialQuery?: string | undefined;
}) {
  const [selectedOperators, setSelectedOperators] = useState<string[]>(() => {
    const known = new Set(lines.flatMap((line) => line.operators));
    return Array.from(new Set(initialOperators ?? [])).filter((code) => known.has(code));
  });
  const [selectedCountries, setSelectedCountries] = useState<Country[]>([]);
  const [statusGroupFilter, setStatusGroupFilter] = useState<SeverityGroup | null>(initialStatusGroup ?? null);
  const [nameQuery, setNameQuery] = useState(initialQuery ?? '');
  // Task 3.4.2: defaults to name order rather than the catalogue's own
  // (arbitrary-looking) ordering, so a first-time visitor to a 125-row
  // table lands somewhere navigable instead of needing to discover sorting
  // exists first.
  const [sort, setSort] = useState<SortState | null>({ field: 'name', direction: 'asc' });
  const countryLabelId = useId();
  const statusLabelId = useId();

  // 2026-09-22 UX review §3.2: the URL stays truthful for refresh, share
  // and Back, now for the search and operator filter too (they used to
  // reset on every reload). `history.replaceState` rather than
  // `router.replace`: Next.js keeps `useSearchParams` in sync with it, and
  // it does not re-run this page's server render the way `router.replace`
  // did on every chip tap (which refetched every line and status, and could
  // reshuffle the table under the visitor's finger). Replace, not push: a
  // filter change is not a new place to go Back to. Debounced, and skipped
  // on mount so landing on the page never rewrites its own URL.
  const urlSyncedRef = useRef(false);
  useEffect(() => {
    if (!urlSyncedRef.current) {
      urlSyncedRef.current = true;
      return;
    }
    const timer = window.setTimeout(() => {
      const next = linesFilterUrl(window.location.href, {
        query: nameQuery,
        operators: selectedOperators,
        statusGroup: statusGroupFilter,
      });
      try {
        window.history.replaceState(null, '', next);
      } catch {
        // Over the browser's replaceState rate limit: the table is still
        // right, only the address bar lags until the next change.
      }
    }, URL_SYNC_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [nameQuery, selectedOperators, statusGroupFilter]);

  function clearPanelFilters() {
    setSelectedOperators([]);
    setSelectedCountries([]);
    setStatusGroupFilter(null);
  }

  function clearEverything() {
    clearPanelFilters();
    setNameQuery('');
  }

  // §3.2's own "let the title reflect it": `app/lines/page.tsx`'s
  // `generateMetadata` is static (its own comment explains why -- a
  // pre-filtered link is not the one people paste), so it cannot know
  // about client-side filter changes at all. This is therefore the one
  // place that can keep the browser tab's title honest about what's
  // actually on screen once a chip (or a dashboard tile landing here) has
  // filtered the table. The unfiltered title is captured once, on mount,
  // so clearing the filter restores EXACTLY what the server rendered
  // rather than a hand-rolled guess at it.
  const unfilteredTitleRef = useRef<string | null>(null);
  const nameByCode = useMemo(() => new Map(tocs.map((toc) => [toc.code, toc.name])), [tocs]);
  const title = filteredTitle(
    statusGroupFilter,
    selectedOperators.map((code) => nameByCode.get(code) ?? code),
    nameQuery,
  );
  useEffect(() => {
    unfilteredTitleRef.current ??= document.title;
    document.title = title ? fullTitle(title) : unfilteredTitleRef.current;
  }, [title]);

  const reportsById = useMemo(() => new Map(reports.map((report) => [report.id, report])), [reports]);
  const pinnedSet = useMemo(() => new Set(pinnedLineIds), [pinnedLineIds]);
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
    // AND-combined with the other three filters, same posture: a
    // severity-group filter answers a different question (what state is
    // the line in right now) than operator/country/name do. `row.worst` is
    // `undefined` for a line with no computed status at all ("No data" in
    // the table below) -- such a row cannot match any specific bucket, and
    // is only ever visible under "All statuses".
    if (statusGroupFilter) {
      result = result.filter(
        (row) => row.worst !== undefined && severityGroup(row.worst.statusSeverity) === statusGroupFilter,
      );
    }
    return result;
  }, [rows, selectedOperators, selectedCountries, statusGroupFilter, nameQuery]);

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

  const activeFilterCount = selectedOperators.length + selectedCountries.length + (statusGroupFilter ? 1 : 0);
  const filtersOrSearchActive =
    nameQuery.trim().length > 0 || selectedOperators.length > 0 || selectedCountries.length > 0;
  const shown = sortedRows.length;
  const summaryVisible = statusGroupFilter !== null || filtersOrSearchActive;
  const shownLabel = `${shown} line${shown === 1 ? '' : 's'}`;
  // The status-only wording predates the search and operator summary and
  // is what the dashboard's counter tiles land on, so it stays.
  const resultSummary =
    statusGroupFilter && !filtersOrSearchActive
      ? shown === 0
        ? `No lines are ${SEVERITY_GROUP_LABELS[statusGroupFilter]} right now.`
        : `${shownLabel} with ${SEVERITY_GROUP_LABELS[statusGroupFilter]}.`
      : shown === 0
        ? 'No lines match.'
        : `${shownLabel} ${shown === 1 ? 'matches' : 'match'}.`;

  function toggleSort(field: SortField) {
    setSort((prev) => {
      if (prev?.field !== field) return { field, direction: 'asc' };
      return { field, direction: prev.direction === 'asc' ? 'desc' : 'asc' };
    });
  }

  return (
    <Stack gap="md" className="linesTableRoot">
      <LinesFilterBar
        query={nameQuery}
        onQueryChange={setNameQuery}
        activeCount={activeFilterCount}
        matchCount={sortedRows.length}
        onClearFilters={clearPanelFilters}
      >
        <MultiSelect
          label="Operator"
          placeholder="All operators"
          data={operatorOptions}
          value={selectedOperators}
          onChange={setSelectedOperators}
          searchable
          clearable
          clearButtonProps={{ 'aria-label': 'Clear operator filter' }}
          nothingFoundMessage={noOptionsFound('No matching operators')}
          // Inside the filters popover or sheet, not portalled: a portalled
          // dropdown counts as "outside" the popover, so picking an option
          // closed the whole panel, and it would sit outside the sheet's
          // focus trap.
          comboboxProps={{ withinPortal: false }}
        />
        <Stack gap={4}>
          <Text id={statusLabelId} size="xs" fw={600} c="dimmed">
            {statusChipLabel(statusGroupFilter)}
          </Text>
          <ChipGroup
            value={statusGroupFilter ?? ''}
            onChange={(value) => setStatusGroupFilter(value === '' ? null : value)}
          >
            <Group gap="xs" role="group" aria-labelledby={statusLabelId}>
              <Chip value="" size="sm" variant={statusGroupFilter === null ? 'filled' : 'outline'}>
                All statuses
              </Chip>
              {SEVERITY_GROUPS_BY_RANK.map((group) => (
                <Chip key={group} value={group} size="sm" variant={statusGroupFilter === group ? 'filled' : 'outline'}>
                  {groupChipLabel(group)}
                </Chip>
              ))}
            </Group>
          </ChipGroup>
        </Stack>
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
            <ChipGroup multiple value={selectedCountries} onChange={(value) => setSelectedCountries(value)}>
              <Group gap="xs" role="group" aria-labelledby={countryLabelId}>
                {countryOptions.map((country) => (
                  <Chip
                    key={country}
                    value={country}
                    size="sm"
                    variant={selectedCountries.includes(country) ? 'filled' : 'outline'}
                  >
                    {COUNTRY_LABELS[country]}
                  </Chip>
                ))}
              </Group>
            </ChipGroup>
          </Stack>
        )}
      </LinesFilterBar>
      {/* 2026-09-22 UX review §3.3: a filtered view says how many rows are
          left and offers a way back, including the zero-row case a
          `0`-count dashboard tile (or a search with no match) can produce.
          A real click handler, not a link: the filters are client state,
          so a plain `<Link href="/lines">` would change the address bar
          without the mounted table re-reading it.

          The count is announced from an always-mounted, visually hidden
          `role="status"` region, so a screen reader hears it change as the
          visitor types or filters (a live region inserted together with its
          text is often not announced). Visually hidden is absolutely
          positioned, so it adds no Stack gap when empty. The visible copy
          of the sentence is `aria-hidden` so it isn't read twice. */}
      <VisuallyHidden role="status">{summaryVisible ? resultSummary : ''}</VisuallyHidden>
      {summaryVisible && (
        <Text size="sm" c="dimmed">
          <span aria-hidden="true">{resultSummary} </span>
          <UnstyledButton onClick={clearEverything} className="textButton">
            Show all lines
          </UnstyledButton>
        </Text>
      )}
      {/* No `TableScrollContainer` (2026-10-14): its `overflow-x: auto` made
          a scroll container, which stops the sticky header below from
          sticking to the page. It isn't needed either. From `sm` up the
          content is at least 728px wide, well over the 420px five columns
          need, and below `sm` the table is three columns (Avg delay and
          Cancelled fold into the Name cell's sub-line, see `hiddenFrom`
          below) that fit even a 320px screen once long names can wrap
          (`breakableLineName`, and `.linesNameCell`/`.linesTableWrap` in
          app/globals.css), so the page itself never scrolls sideways
          (e2e/nav.spec.ts's `hasHorizontalOverflow`).

          `stickyHeader`: the column headings stick under the sticky search
          bar, offset by the bar's own height (`--lines-filter-bar-height`,
          which also sizes the bar, so the two can't drift apart). */}
      <Box className="linesTableWrap">
        <Table stickyHeader stickyHeaderOffset="var(--lines-filter-bar-height)">
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
              {/* Task 3.4.7: `white-space: nowrap` so "Avg delay ↕" (the
                longest of the four labels) can't wrap its own sort glyph
                onto a second line, orphaning it away from the label it
                belongs to. */}
              <TableTh aria-sort={ariaSort('name', sort)} className="sortHeader">
                <UnstyledButton onClick={() => toggleSort('name')} className="sortHeader__button">
                  Name
                  <SortGlyph field="name" sort={sort} />
                </UnstyledButton>
              </TableTh>
              <TableTh aria-sort={ariaSort('status', sort)} className="sortHeader">
                <UnstyledButton onClick={() => toggleSort('status')} className="sortHeader__button">
                  Status
                  <SortGlyph field="status" sort={sort} />
                </UnstyledButton>
              </TableTh>
              <TableTh aria-sort={ariaSort('avgDelay', sort)} visibleFrom="sm" className="sortHeader">
                <UnstyledButton onClick={() => toggleSort('avgDelay')} className="sortHeader__button">
                  Avg delay
                  <SortGlyph field="avgDelay" sort={sort} />
                </UnstyledButton>
              </TableTh>
              <TableTh aria-sort={ariaSort('cancelled', sort)} visibleFrom="sm" className="sortHeader">
                <UnstyledButton onClick={() => toggleSort('cancelled')} className="sortHeader__button">
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
                <TableTd className="linesNameCell">
                  {/* Task 3.4.8: at narrow widths a wrapped two-line name read
                    as two separate stacked list items rather than one
                    wrapped link, at Mantine `Text`'s default (looser)
                    line-height. */}
                  <TextLink href={`/lines/${line.id}`} lh={1.3}>
                    {breakableLineName(line.name)}
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
                    // rule) -- otherwise "No data" clips to "N…" in this same
                    // table at narrow widths.
                    <Tooltip label={noStatusReason(representative)}>
                      <span>
                        <StateBadge tone="neutral" data-status-badge="">
                          No data
                        </StateBadge>
                      </span>
                    </Tooltip>
                  )}
                </TableTd>
                <TableTd visibleFrom="sm">
                  {stats ? (
                    <Text size="sm">{stats.avgDelayMinutes.toFixed(1)} min</Text>
                  ) : representative ? (
                    <Tooltip label={sampleUnavailableReason(representative)}>{NO_VALUE}</Tooltip>
                  ) : (
                    NO_VALUE
                  )}
                </TableTd>
                <TableTd visibleFrom="sm">
                  {cancelledPct !== null ? (
                    <Text size="sm">{cancelledPct}%</Text>
                  ) : representative ? (
                    <Tooltip label={sampleUnavailableReason(representative)}>{NO_VALUE}</Tooltip>
                  ) : (
                    NO_VALUE
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
      </Box>
    </Stack>
  );
}
