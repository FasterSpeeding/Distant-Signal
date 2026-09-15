'use client';

import { useState, type FormEvent } from 'react';
import {
  Alert,
  Badge,
  Button,
  Group,
  MultiSelect,
  NumberInput,
  ScrollArea,
  SegmentedControl,
  Select,
  Stack,
  Text,
} from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentSearchResponse, IncidentSummary, LineSummary, Suggestion } from '@/lib/types';

type DatePreset = '7d' | '30d' | '90d' | 'all';

function calendarDaysAgo(days: number): string {
  return dayjs().subtract(days, 'day').format('YYYY-MM-DD');
}

/** Exactly one of three mutually-exclusive states, mirroring
 * `TrainSearchForm.tsx`'s own `Results` type -- `nextCursor` lives INSIDE
 * the success variant for the same reason it does there: it must not
 * survive a state transition (a fresh search, or an error) it does not
 * belong to. */
type Results = { rows: IncidentSummary[]; nextCursor: string | null } | 'error' | null;

/** `/incidents`'s one interactive component: filter form plus a
 * cursor-paginated, "Load more"-driven results list over
 * `GET /public/incidents`. Mirrors `TrainSearchForm.tsx`'s client-side
 * fetch/`useState`/"Load more" shape exactly (Decision 6 of
 * docs/superpowers/specs/2026-09-12-incident-archive-design.md), not the
 * `HistoryRangePicker`/server-searchParams shape `/lines/[id]/history` uses
 * -- this filter set (six independent optional filters) is a closer match
 * to `TrainSearchForm`'s multi-filter interactive search than to that range
 * picker's single from/to control.
 *
 * `lines`/`tocs` are fetched once, server-side, by `app/incidents/page.tsx`
 * and passed down as props -- the same "reference data fetched once by the
 * page" shape `AllLinesPage`/`AllLinesTable` already establishes for `tocs`.
 * `lines` is filtered to catalogue lines only (`source === 'catalogue'`)
 * before it is ever offered as a filter option, matching the backend's own
 * scoping (Decision 2): there is no way to even attempt filtering by a
 * private custom line from this form.
 *
 * Defaults to a 30-day `from` floor on first load when no initial filters
 * are supplied -- an unfiltered "all incidents ever ingested" default view
 * is the direct equivalent of the list-spamminess failure mode this
 * codebase's own research already diagnosed for `/lines/[id]/history`'s
 * Timeline tab, just multiplied across the whole network. "All time" stays
 * one preset click away; this is a default, not a ceiling. */
export function IncidentSearchForm({
  lines,
  tocs,
  initialOperator = '',
  initialLine = '',
  initialFrom = '',
  initialTo = '',
}: {
  lines: LineSummary[];
  tocs: Suggestion[];
  initialOperator?: string;
  initialLine?: string;
  initialFrom?: string;
  initialTo?: string;
}) {
  const catalogueLines = lines.filter((line) => line.source === 'catalogue');

  const [operators, setOperators] = useState<string[]>(
    initialOperator ? initialOperator.split(',').filter(Boolean) : [],
  );
  const [lineId, setLineId] = useState<string | null>(initialLine || null);
  const [fromDate, setFromDate] = useState<string | null>(
    initialFrom ? initialFrom.slice(0, 10) : calendarDaysAgo(30),
  );
  const [toDate, setToDate] = useState<string | null>(initialTo ? initialTo.slice(0, 10) : null);
  const [preset, setPreset] = useState<DatePreset | null>(initialFrom ? null : '30d');
  const [plannedFilter, setPlannedFilter] = useState<'all' | 'planned' | 'realtime'>('all');
  const [clearedFilter, setClearedFilter] = useState<'all' | 'active' | 'cleared'>('all');
  const [priorityMin, setPriorityMin] = useState<number | ''>('');
  const [priorityMax, setPriorityMax] = useState<number | ''>('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);

  const priorityValid = priorityMin === '' || priorityMax === '' || priorityMin <= priorityMax;

  function applyPreset(next: DatePreset) {
    setPreset(next);
    if (next === 'all') {
      setFromDate(null);
      setToDate(null);
      return;
    }
    const days = next === '7d' ? 7 : next === '30d' ? 30 : 90;
    setFromDate(calendarDaysAgo(days));
    setToDate(null);
  }

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
  function searchParamsFor() {
    const params = new URLSearchParams();
    if (operators.length > 0) params.set('operator', operators.join(','));
    if (lineId) params.set('line', lineId);
    if (fromDate) params.set('from', new Date(fromDate).toISOString());
    if (toDate) params.set('to', new Date(toDate).toISOString());
    if (plannedFilter === 'planned') params.set('planned', 'true');
    if (plannedFilter === 'realtime') params.set('planned', 'false');
    if (clearedFilter === 'active') params.set('cleared', 'false');
    if (clearedFilter === 'cleared') params.set('cleared', 'true');
    if (priorityMin !== '') params.set('priority_min', String(priorityMin));
    if (priorityMax !== '') params.set('priority_max', String(priorityMax));
    return params;
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!priorityValid || searching) return;
    setSearching(true);
    try {
      const response = await fetch(`/api/incidents?${searchParamsFor().toString()}`);
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: IncidentSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleLoadMore() {
    if (results === null || results === 'error') return;
    if (results.nextCursor === null || loadingMore) return;
    setLoadingMore(true);
    try {
      const params = searchParamsFor();
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/incidents?${params.toString()}`);
      if (!response.ok) {
        setResults((current) =>
          current !== null && current !== 'error' ? { rows: current.rows, nextCursor: null } : current,
        );
        return;
      }
      const body: IncidentSearchResponse = await response.json();
      setResults((current) =>
        current !== null && current !== 'error'
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor }
          : current,
      );
    } catch {
      setResults((current) =>
        current !== null && current !== 'error' ? { rows: current.rows, nextCursor: null } : current,
      );
    } finally {
      setLoadingMore(false);
    }
  }

  function resultsContent() {
    if (searching) {
      return (
        <Text size="sm" c="dimmed">
          Searching…
        </Text>
      );
    }
    if (results === null) {
      return (
        <Text size="sm" c="dimmed">
          Press Search to browse incidents across the network.
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search incidents right now. Try again.
        </Alert>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No incidents match these filters.
        </Text>
      );
    }
    return (
      <>
        <ScrollArea mah={520} offsetScrollbars>
          <Stack gap="sm">
            {results.rows.map((row) => (
              <Stack key={row.incidentId} gap={4}>
                <Group justify="space-between" wrap="nowrap">
                  <TextLink href={`/incidents/${encodeURIComponent(row.incidentId)}`} underline="always">
                    {row.summary}
                  </TextLink>
                  <Text size="xs" c="dimmed">
                    {formatDateTime(row.firstSeenAt)}
                  </Text>
                </Group>
                <Group gap="xs">
                  <Badge color={row.isPlanned ? 'blue' : 'orange'}>
                    {row.isPlanned ? 'Planned Work' : 'Real-Time'}
                  </Badge>
                  <Badge color={row.isCleared ? 'gray' : 'green'}>{row.isCleared ? 'Cleared' : 'Active'}</Badge>
                  {row.operators.map((code) => (
                    <Badge key={code} variant="outline" color="grape">
                      {code}
                    </Badge>
                  ))}
                  {row.affectedStations.map((crs) => (
                    <Badge key={crs} variant="outline" color="gray">
                      {crs}
                    </Badge>
                  ))}
                </Group>
              </Stack>
            ))}
          </Stack>
        </ScrollArea>
        {results.nextCursor !== null && (
          <Group>
            <Button variant="default" size="xs" onClick={handleLoadMore} disabled={loadingMore} loading={loadingMore}>
              Load more
            </Button>
          </Group>
        )}
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <MultiSelect
        label="Operator (optional)"
        placeholder="Any operator"
        description="Matches an incident whose operators overlap any of these -- not 'scoped to exactly this operator.'"
        data={tocs.map((toc) => ({ value: toc.code, label: `${toc.code} — ${toc.name}` }))}
        value={operators}
        onChange={setOperators}
        searchable
        clearable
      />
      <Select
        label="Line (optional)"
        placeholder="Any line"
        description="Incidents affecting stations on this line -- a station-overlap approximation, not a real line match. It can miss incidents that only matched a line by keyword or shared operator, with no station in common."
        data={catalogueLines.map((line) => ({ value: line.id, label: line.name }))}
        value={lineId}
        onChange={setLineId}
        searchable
        clearable
      />
      <Group gap="sm">
        <Button variant={preset === '7d' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('7d')}>
          7 days
        </Button>
        <Button variant={preset === '30d' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('30d')}>
          30 days
        </Button>
        <Button variant={preset === '90d' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('90d')}>
          90 days
        </Button>
        <Button variant={preset === 'all' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('all')}>
          All time
        </Button>
      </Group>
      <Group align="end">
        <DatePickerInput
          label="From (optional)"
          value={fromDate}
          onChange={(value) => {
            setFromDate(value);
            setPreset(null);
          }}
          clearable
        />
        <DatePickerInput
          label="To (optional)"
          value={toDate}
          onChange={(value) => {
            setToDate(value);
            setPreset(null);
          }}
          clearable
        />
      </Group>
      <SegmentedControl
        value={plannedFilter}
        onChange={(value) => setPlannedFilter(value as 'all' | 'planned' | 'realtime')}
        data={[
          { label: 'All', value: 'all' },
          { label: 'Planned work', value: 'planned' },
          { label: 'Real-time', value: 'realtime' },
        ]}
      />
      <SegmentedControl
        value={clearedFilter}
        onChange={(value) => setClearedFilter(value as 'all' | 'active' | 'cleared')}
        data={[
          { label: 'All', value: 'all' },
          { label: 'Active', value: 'active' },
          { label: 'Cleared', value: 'cleared' },
        ]}
      />
      <Group grow align="flex-start">
        <NumberInput
          label="Priority (raw feed value — meaning undocumented)"
          description="Minimum, inclusive."
          value={priorityMin}
          onChange={(value) => setPriorityMin(typeof value === 'number' ? value : '')}
        />
        <NumberInput
          label="Priority (raw feed value — meaning undocumented)"
          description="Maximum, inclusive."
          value={priorityMax}
          onChange={(value) => setPriorityMax(typeof value === 'number' ? value : '')}
          error={!priorityValid ? 'Minimum must not exceed maximum' : null}
        />
      </Group>
      <Text size="xs" c="dimmed">
        Priority is a raw feed value from the Knowledgebase incident data with no documented
        &quot;major&quot;/&quot;minor&quot; meaning — shown as-is, not a severity scale.
      </Text>
      <Group>
        <Button type="submit" disabled={!priorityValid || searching}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
    </Stack>
  );
}
