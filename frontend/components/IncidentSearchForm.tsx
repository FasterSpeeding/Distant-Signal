'use client';

import { useId, useMemo, useState, type ReactNode } from 'react';
import { Chip, ChipGroup, Group, InputWrapper, MultiSelect, NumberInput, Select, Stack, Text } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { londonCalendarDay, londonDayEndIso, londonDayStartIso } from '@/lib/londonWallClock';
import { IncidentResults } from './IncidentResults';
import { noOptionsFound } from './noOptionsFound';
import { countLabel, ListFilterBar, type ListNoun } from './ListFilterBar';
import { ListResultSummary } from './ListResultSummary';
import type { IncidentSearchResponse, LineSummary, Suggestion } from '@/lib/types';
import {
  activeIncidentFilterCount,
  calendarDaysAgo,
  INCIDENT_URL_PARAMS,
  incidentSearchParams,
  incidentsTitle,
  incidentUrlParams,
  initialIncidentPeriod,
  initialStateFilter,
  parsePriorityParam,
  PRESET_DAYS,
  type DatePreset,
  type IncidentFilters,
  type IncidentResults as IncidentResultsState,
  type StateFilter,
} from '@/lib/incidentSearch';
import { useLiveSearch } from '@/lib/useLiveSearch';
import { paramValue, useInitialSearchParams } from '@/lib/useInitialSearchParams';
import { useUrlFilterSync } from '@/lib/useUrlFilterSync';
import { useFilteredDocumentTitle } from '@/lib/useFilteredDocumentTitle';

const INCIDENT_NOUN: ListNoun = { one: 'incident', other: 'incidents' };

/** One labelled row of single-select chips: Period, Type and Status. Chips
 * rather than a segmented control because they wrap, so five periods fit
 * the filters popover and a phone's bottom sheet alike. */
function ChipRow<T extends string>({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: T;
  onChange: (next: T) => void;
  options: readonly { value: T; label: string }[];
}) {
  const labelId = useId();
  return (
    <Stack gap={4}>
      <Text id={labelId} size="xs" fw={600} c="dimmed">
        {label}
      </Text>
      <ChipGroup value={value} onChange={onChange}>
        <Group gap="xs" role="radiogroup" aria-labelledby={labelId}>
          {options.map((option) => (
            <Chip
              key={option.value}
              value={option.value}
              size="sm"
              variant={value === option.value ? 'filled' : 'outline'}
            >
              {option.label}
            </Chip>
          ))}
        </Group>
      </ChipGroup>
    </Stack>
  );
}

/** `/incidents`' one interactive component: "Find an incident" and a
 * "Filters" panel (`ListFilterBar`, docs/style-guide.md "List search and
 * filters") over a cursor-paginated, "Load more" results list from
 * `GET /public/incidents`.
 *
 * Filters apply live: every change is fetched after a short pause
 * (`useLiveSearch`), with a spinner in the search box while it runs and the
 * rows on screen left until the new ones arrive. The filter state lives in
 * the URL (`useUrlFilterSync`, `history.replaceState`), so following a
 * result to `/incidents/[incidentId]` and pressing Back restores both the
 * filters and the search, and the page's server render (which fetches the
 * line and operator lists) never re-runs on a filter change.
 *
 * `lines`/`tocs` are fetched once, server-side, by `app/incidents/page.tsx`.
 * `lines` is filtered to catalogue lines only (`source === 'catalogue'`)
 * before it is offered as a filter option, matching the backend's own
 * scoping: there is no way to filter by a private custom line here.
 *
 * Defaults to the last 30 days: an unfiltered "every incident ever
 * ingested" first view would be the list-spamminess failure mode this
 * codebase's own research diagnosed for `/lines/[id]/history`, multiplied
 * across the whole network. "All time" stays one chip away.
 *
 * Known, accepted limitation: after "Load more", a reload or Back restores
 * page 1 of the same search only. `nextCursor` is server-issued paging
 * state with no place in the URL. */
export function IncidentSearchForm({
  lines,
  tocs,
  initialQuery: initialQueryProp,
  initialOperator: initialOperatorProp,
  initialLine: initialLineProp,
  initialFrom: initialFromProp,
  initialTo: initialToProp,
  initialPeriod: initialPeriodProp,
  initialPlanned: initialPlannedProp,
  initialState: initialStateProp,
  initialCleared: initialClearedProp,
  initialPriorityMin: initialPriorityMinProp,
  initialPriorityMax: initialPriorityMaxProp,
}: {
  lines: LineSummary[];
  tocs: Suggestion[];
  /** `?q=`, "Find an incident". */
  initialQuery?: string | undefined;
  initialOperator?: string | undefined;
  initialLine?: string | undefined;
  initialFrom?: string | undefined;
  initialTo?: string | undefined;
  /** `?period=`: `7d`, `30d`, `90d`, `all` or `custom` (see
   * `initialIncidentPeriod`). */
  initialPeriod?: string | undefined;
  initialPlanned?: string | undefined;
  /** `?state=active|ended|cleared`, the Status filter's own URL form. */
  initialState?: string | undefined;
  /** Legacy `?cleared=true|false` (links shared before `state` existed):
   * read only when `initialState` is absent. */
  initialCleared?: string | undefined;
  initialPriorityMin?: string | undefined;
  initialPriorityMax?: string | undefined;
}) {
  // Seeded from the address bar, not the server props: see
  // `useInitialSearchParams` (after Back, the props are stale).
  const urlParams = useInitialSearchParams();
  const seed = (key: string, prop: string | undefined) => (urlParams ? paramValue(urlParams, key) : prop);
  const initialQuery = seed('q', initialQueryProp) ?? '';
  const initialOperator = seed('operator', initialOperatorProp) ?? '';
  const initialLine = seed('line', initialLineProp) ?? '';
  const initialFrom = seed('from', initialFromProp);
  const initialTo = seed('to', initialToProp);
  const initialPeriod = seed('period', initialPeriodProp);
  const initialPlanned = seed('planned', initialPlannedProp);
  const initialState = seed('state', initialStateProp);
  const initialCleared = seed('cleared', initialClearedProp);
  const initialPriorityMin = seed('priority_min', initialPriorityMinProp);
  const initialPriorityMax = seed('priority_max', initialPriorityMaxProp);
  const catalogueLines = useMemo(() => lines.filter((line) => line.source === 'catalogue'), [lines]);
  /** Line id -> display name, so a result row's `affectedLines` renders as
   * "Elizabeth line" rather than "elizabeth-line". An id with no entry (a
   * line retired from the catalogue since the incident was ingested) falls
   * back to the raw id rather than disappearing. */
  const lineNamesById = useMemo(() => new Map(catalogueLines.map((line) => [line.id, line.name])), [catalogueLines]);
  const tocNamesByCode = useMemo(() => new Map(tocs.map((toc) => [toc.code, toc.name])), [tocs]);

  const [initialDates] = useState(() =>
    initialIncidentPeriod(initialPeriod, initialFrom, initialTo, londonCalendarDay),
  );
  const [query, setQuery] = useState(initialQuery);
  const [operators, setOperators] = useState<string[]>(
    initialOperator ? initialOperator.split(',').filter(Boolean) : [],
  );
  const [lineId, setLineId] = useState<string | null>(initialLine || null);
  const [fromDate, setFromDate] = useState<string | null>(initialDates.fromDate);
  const [toDate, setToDate] = useState<string | null>(initialDates.toDate);
  const [preset, setPreset] = useState<DatePreset | null>(initialDates.preset);
  const [plannedFilter, setPlannedFilter] = useState<IncidentFilters['planned']>(
    initialPlanned === 'true' ? 'planned' : initialPlanned === 'false' ? 'realtime' : 'all',
  );
  const [stateFilter, setStateFilter] = useState<StateFilter>(initialStateFilter(initialState, initialCleared));
  const [priorityMin, setPriorityMin] = useState<number | ''>(parsePriorityParam(initialPriorityMin));
  const [priorityMax, setPriorityMax] = useState<number | ''>(parsePriorityParam(initialPriorityMax));
  const [results, setResults] = useState<IncidentResultsState>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  const priorityValid = priorityMin === '' || priorityMax === '' || priorityMin <= priorityMax;

  const filters: IncidentFilters = {
    query,
    operators,
    lineId,
    fromDate,
    toDate,
    preset,
    planned: plannedFilter,
    state: stateFilter,
    priorityMin,
    priorityMax,
  };
  const apiQuery = priorityValid ? incidentSearchParams(filters, londonDayStartIso, londonDayEndIso).toString() : null;

  const { pending } = useLiveSearch(apiQuery, async (searched, signal) => {
    // A function, not `signal.aborted` inline: it changes across the awaits.
    const superseded = () => signal.aborted;
    try {
      const response = await fetch(`/api/incidents?${searched}`, { signal });
      if (superseded()) return;
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body = (await response.json()) as IncidentSearchResponse;
      if (superseded()) return;
      setResults({
        rows: body.results,
        nextCursor: body.nextCursor,
        query: searched,
        loadMoreFailed: false,
        total: typeof body.total === 'number' ? body.total : null,
      });
    } catch {
      if (!superseded()) setResults('error');
    }
  });

  useUrlFilterSync(filters, (current, state) => {
    const url = new URL(current);
    for (const key of INCIDENT_URL_PARAMS) url.searchParams.delete(key);
    for (const [key, value] of incidentUrlParams(state, londonDayStartIso, londonDayEndIso)) {
      url.searchParams.append(key, value);
    }
    const search = url.searchParams.toString();
    return `${url.pathname}${search ? `?${search}` : ''}${url.hash}`;
  });

  useFilteredDocumentTitle(incidentsTitle(filters, lineNamesById, tocNamesByCode));

  function handlePeriodChange(next: DatePreset | 'custom') {
    if (next === 'custom') {
      // No date reset: this only reveals the date fields for whatever
      // from/to they already held.
      setPreset(null);
      return;
    }
    setPreset(next);
    setToDate(null);
    setFromDate(next === 'all' ? null : calendarDaysAgo(PRESET_DAYS[next]));
  }

  function clearPanelFilters() {
    setOperators([]);
    setLineId(null);
    handlePeriodChange('30d');
    setPlannedFilter('all');
    setStateFilter('all');
    setPriorityMin('');
    setPriorityMax('');
  }

  const activeCount = activeIncidentFilterCount(filters);
  const searchActive = query.trim() !== '';

  async function handleLoadMore() {
    if (results === null || results === 'error') return;
    if (results.nextCursor === null || loadingMore) return;
    // The exact result-set object this page is a continuation of. A new
    // search can resolve first and replace it; the live search always
    // installs a brand-new object, so identity is all that is needed to
    // spot that. Without this check the stale page would append its rows to
    // (and stamp its cursor, or its failure, onto) somebody else's search.
    const pagedFrom = results;
    setLoadingMore(true);
    try {
      // Rebuilt from the search that produced these rows (`results.query`),
      // never the live filters: a filter changed since then must not mix
      // into a page fetched with this cursor.
      const params = new URLSearchParams(results.query);
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/incidents?${params.toString()}`);
      if (!response.ok) {
        // The cursor is kept, not nulled: this page just failed to load, and
        // the reader gets a named error plus a working retry rather than a
        // list that quietly stops one page short of the end.
        setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
        return;
      }
      const body = (await response.json()) as IncidentSearchResponse;
      setResults((current) =>
        current === pagedFrom
          ? {
              ...current,
              rows: [...current.rows, ...body.results],
              nextCursor: body.nextCursor,
              loadMoreFailed: false,
            }
          : current,
      );
    } catch {
      setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
    } finally {
      setLoadingMore(false);
    }
  }

  const total = results !== null && results !== 'error' ? results.total : null;
  const summary =
    total === null
      ? ''
      : total === 0
        ? 'No incidents match.'
        : `${countLabel(total, INCIDENT_NOUN)} ${total === 1 ? 'matches' : 'match'}.`;
  // Every `clearable` field below carries an explicit `clearButtonProps`
  // aria-label (Mantine's own clear button has no accessible name, which
  // axe's `button-name` flags) and `iconHitArea24` (globals.css), which pads
  // the 22px button to the 24px WCAG target-size floor.
  //
  // Option lists and the date picker render inside the panel
  // (`withinPortal: false`): a portalled dropdown counts as outside the
  // popover, so picking an option closed the whole panel, and it would sit
  // outside the sheet's focus trap.
  const panelFields: ReactNode = (
    <>
      <MultiSelect
        label="Operator"
        placeholder="Any operator"
        data={tocs.map((toc) => ({ value: toc.code, label: `${toc.code} · ${toc.name}` }))}
        value={operators}
        onChange={setOperators}
        searchable
        clearable
        // Mantine hides the whole dropdown when it has no options and no
        // `nothingFoundMessage`, leaving the input `aria-expanded="true"`
        // over nothing.
        nothingFoundMessage={noOptionsFound('No matching operators')}
        clearButtonProps={{ 'aria-label': 'Clear operator filter', className: 'iconHitArea24' }}
        comboboxProps={{ withinPortal: false }}
      />
      <Select
        label="Line"
        placeholder="Any line"
        description="Matched the same way as the line's status page."
        data={catalogueLines.map((line) => ({ value: line.id, label: line.name }))}
        value={lineId}
        onChange={setLineId}
        searchable
        clearable
        nothingFoundMessage={noOptionsFound('No matching lines')}
        clearButtonProps={{ 'aria-label': 'Clear line filter', className: 'iconHitArea24' }}
        comboboxProps={{ withinPortal: false }}
      />
      <ChipRow
        label="Period"
        value={preset ?? 'custom'}
        onChange={handlePeriodChange}
        options={[
          { label: '7 days', value: '7d' },
          { label: '30 days', value: '30d' },
          { label: '90 days', value: '90d' },
          { label: 'All time', value: 'all' },
          { label: 'Custom dates', value: 'custom' },
        ]}
      />
      {preset === null && (
        <Group grow align="flex-start">
          <DatePickerInput
            valueFormat="D MMM YYYY"
            label="From"
            placeholder="Any"
            value={fromDate}
            onChange={setFromDate}
            clearable
            clearButtonProps={{ 'aria-label': 'Clear the from date', className: 'iconHitArea24' }}
            popoverProps={{ withinPortal: false }}
          />
          <DatePickerInput
            valueFormat="D MMM YYYY"
            label="To"
            placeholder="Any"
            value={toDate}
            onChange={setToDate}
            clearable
            clearButtonProps={{ 'aria-label': 'Clear the to date', className: 'iconHitArea24' }}
            popoverProps={{ withinPortal: false }}
          />
        </Group>
      )}
      <ChipRow
        label="Type"
        value={plannedFilter}
        onChange={setPlannedFilter}
        options={[
          { label: 'All', value: 'all' },
          { label: 'Planned work', value: 'planned' },
          { label: 'Real-time', value: 'realtime' },
        ]}
      />
      <ChipRow
        label="Status"
        value={stateFilter}
        onChange={setStateFilter}
        options={[
          { label: 'All', value: 'all' },
          { label: 'Active', value: 'active' },
          { label: 'Ended', value: 'ended' },
          { label: 'Cleared', value: 'cleared' },
        ]}
      />
      {/* Review §2.14: the raw-feed caveat is said once, as the wrapper's
          description. The feed has no documented "major"/"minor" meaning
          for priority, so it stays visibly unprocessed. */}
      <InputWrapper
        label="Priority range"
        description="The feed's own number, shown as is. It has no documented meaning."
        error={!priorityValid ? 'Minimum must not exceed maximum' : null}
      >
        <Group grow align="flex-start">
          <NumberInput
            hideControls
            label="Minimum"
            value={priorityMin}
            onChange={(value) => setPriorityMin(typeof value === 'number' ? value : '')}
          />
          <NumberInput
            hideControls
            label="Maximum"
            value={priorityMax}
            onChange={(value) => setPriorityMax(typeof value === 'number' ? value : '')}
          />
        </Group>
      </InputWrapper>
    </>
  );

  return (
    <Stack gap="md">
      <ListFilterBar
        searchLabel="Find an incident"
        query={query}
        onQueryChange={setQuery}
        noun={INCIDENT_NOUN}
        matchCount={pending ? null : total}
        pending={pending}
        activeCount={activeCount}
        onClearFilters={clearPanelFilters}
        panelWidth={380}
        scrollablePanel
      >
        {panelFields}
      </ListFilterBar>
      <ListResultSummary
        visible={(searchActive || activeCount > 0) && !pending && total !== null}
        summary={summary}
        // Back to the default view, which is itself a filter (30 days), so
        // not "Show all incidents".
        resetLabel="Show the last 30 days"
        onReset={() => {
          clearPanelFilters();
          setQuery('');
        }}
        announcement={pending ? 'Searching' : undefined}
      />
      <Stack gap="xs" mih={72}>
        <IncidentResults
          results={results}
          searching={pending && results === null}
          pending={pending}
          loadingMore={loadingMore}
          onLoadMore={() => void handleLoadMore()}
          tocNamesByCode={tocNamesByCode}
          lineNamesById={lineNamesById}
        />
      </Stack>
    </Stack>
  );
}
