'use client';

import { useState, type FormEvent } from 'react';
import {
  Alert,
  Badge,
  Button,
  Group,
  MultiSelect,
  NumberInput,
  SegmentedControl,
  Select,
  Stack,
  Text,
} from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { LoadMoreControl } from './LoadMoreControl';
import { TextLink } from './TextLink';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentSearchResponse, IncidentSummary, LineSummary, Suggestion } from '@/lib/types';

type DatePreset = '7d' | '30d' | '90d' | 'all';

/** How many affected-line badges one result row shows before collapsing the
 * rest into a "+N more". An operator-only incident on a large TOC matches
 * every catalogue line that TOC runs -- Northern alone is 13 -- which would
 * otherwise bury the summary under a wall of badges. */
const MAX_LINE_BADGES = 4;

function calendarDaysAgo(days: number): string {
  return dayjs().subtract(days, 'day').format('YYYY-MM-DD');
}

/** Exactly one of three mutually-exclusive states, mirroring
 * `TrainSearchForm.tsx`'s own `Results` type -- `nextCursor` lives INSIDE
 * the success variant for the same reason it does there: it must not
 * survive a state transition (a fresh search, or an error) it does not
 * belong to. `query` is the exact query string that was submitted to produce
 * these `rows` -- captured once at submit time, mirroring
 * `TrainSearchForm.tsx`'s own capture of `date` for the identical reason:
 * `handleLoadMore` must page through THIS search's filters, never whatever
 * live filter state the form happens to hold when "Load more" is pressed.
 *
 * `loadMoreFailed` records that the LAST "Load more" press errored, so the
 * footer can say so instead of pretending the list ended there. It lives in
 * this variant (rather than its own `useState`) for the same reason
 * `nextCursor` does: a fresh search replaces the whole object, so the flag
 * cannot outlive the result set it describes. The cursor is deliberately
 * kept on failure -- it is still a valid cursor, so the retry the footer
 * offers is a real one. */
type Results =
  | { rows: IncidentSummary[]; nextCursor: string | null; query: string; loadMoreFailed: boolean }
  | 'error'
  | null;

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
  /** Line id -> display name, so a result row's `affectedLines` renders as
   * "Elizabeth line" rather than "elizabeth-line". An id with no entry (a
   * line retired from the catalogue since the incident was ingested) falls
   * back to the raw id rather than disappearing. */
  const lineNamesById = new Map(catalogueLines.map((line) => [line.id, line.name]));

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

  /** `toDate` is a date-only (`YYYY-MM-DD`) value from `DatePickerInput`, and
   * the backend's `to` bound is an inclusive `first_seen_at <= to` comparison
   * (see `crates/api/src/routes/incidents.rs`/
   * `queries::search_incidents`). `new Date(toDate).toISOString()` resolves
   * to UTC midnight at the START of that day, which would make the bound
   * exclude nearly every incident actually first seen on the selected day --
   * contradicting the inclusive "To" framing shown in this form. This names
   * the END of that same UTC calendar day instead, matching the same
   * "date-only string is a UTC calendar day" convention `fromDate` already
   * relies on (`new Date(fromDate).toISOString()` below lands on that day's
   * UTC midnight, i.e. its start). */
  function endOfUtcDay(dateOnly: string): string {
    return `${dateOnly}T23:59:59.999Z`;
  }

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
  function searchParamsFor() {
    const params = new URLSearchParams();
    if (operators.length > 0) params.set('operator', operators.join(','));
    if (lineId) params.set('line', lineId);
    if (fromDate) params.set('from', new Date(fromDate).toISOString());
    if (toDate) params.set('to', endOfUtcDay(toDate));
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
    // Captured synchronously as the exact query string that was submitted --
    // mirroring `TrainSearchForm.tsx`'s own capture of `date` inside its
    // `Results` success variant. `handleLoadMore` below reuses this SAME
    // string for every subsequent page of this result set rather than
    // rebuilding it from live filter state, so a filter changed after
    // searching (but before "Load more" is pressed) cannot silently mix into
    // a page fetched with the original cursor.
    const query = searchParamsFor().toString();
    try {
      const response = await fetch(`/api/incidents?${query}`);
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: IncidentSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor, query, loadMoreFailed: false });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleLoadMore() {
    if (results === null || results === 'error') return;
    if (results.nextCursor === null || loadingMore) return;
    // The exact result-set object this page is a continuation of. Search is
    // not disabled while a page is in flight, so a fresh search can resolve
    // first and leave this response describing a result set that is no
    // longer on screen; `handleSubmit` always installs a BRAND NEW object,
    // so identity is all that is needed to spot that. Without this check the
    // stale page would append its rows to (and stamp its cursor, or its
    // failure, onto) somebody else's search.
    const pagedFrom = results;
    setLoadingMore(true);
    try {
      // Rebuilt from the ORIGINAL search's query string (`results.query`),
      // never from live `searchParamsFor()` -- see `handleSubmit`'s comment.
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
      const body: IncidentSearchResponse = await response.json();
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
        {/* Deliberately NOT wrapped in a `ScrollArea` (`mah`-capped or
         * otherwise). It used to be, and that hard-clipped the archive: a
         * Mantine `ScrollArea` root is `position: relative; overflow: hidden`
         * (`@mantine/core/styles/ScrollArea.css`, `.m_d57069b5`) while its
         * viewport is `height: 100%`. With only `mah` on the root, the root's
         * own `height` stays `auto`, so that `100%` resolves to `auto` too
         * (CSS 2.1 §10.5: a percentage height against a content-sized
         * containing block computes to `auto`) -- the viewport grows to its
         * full content height and therefore never overflows *itself*, so it
         * never scrolls, while the root clamps to the cap and clips
         * everything past it with `overflow: hidden`. Net effect: no pointer,
         * wheel or scrollbar gesture could reach a result past the cap
         * (keyboard focus still could -- browsers scroll an `overflow:
         * hidden` box to reveal a focused descendant -- which left the box
         * parked at an offset with no way back), and each "Load more"
         * appended rows straight into the clipped region. Worse on a ~360px
         * phone, where rows are two or three lines tall so the cap landed
         * after only a handful of them. Nothing hinted that anything had
         * been cut off: Mantine hides the native scrollbar
         * (`scrollbar-width: none`) and draws its own, and that one is
         * sized from `scrollHeight` vs `clientHeight` -- equal here -- so it
         * never appeared either.
         *
         * `ScrollArea.Autosize` IS the Mantine component that supports a max
         * height (it wraps the root in a `display: flex` / `flex: 1` /
         * `overflow: hidden` chain, which is what makes the root's height
         * definite). It is still not used here: letting the page scroll is
         * what `StationTimetable.tsx` -- the other paginated "Load more"
         * list in this app -- already does, rendering its rows as a plain
         * `Stack` with no inner scroll region. (`TrainSearchForm.tsx`, which
         * this component's header says it mirrors, and `TrackTrainForm.tsx`'s
         * departure picker both carried the same `mah`-capped `ScrollArea`
         * and have since been fixed the same way.) A nested scroller buys
         * nothing here anyway --
         * the filter form above is short, so there are no sticky controls to
         * preserve -- while costing real usability on touch, where it steals
         * the page's own scroll gesture. */}
        <Stack gap="sm" data-incident-results>
          {results.rows.map((row) => (
            <Stack key={row.incidentId} gap={4}>
              {/* `wrap` is left at Mantine's wrapping default rather than
               * `nowrap`: at ~360px the summary and the timestamp cannot
               * share a line, and forcing them to shrank the timestamp
               * until it broke mid-value ("19 Aug 2026," etc. across four
               * lines) -- under `nowrap` its floor is `min-width: auto`,
               * i.e. its widest *word*, not the whole value. The timestamp
               * instead stays unbreakable and claims the end of whichever
               * line it lands on. `margin-inline-start: auto` rather than
               * the `Group`'s `justify` because `space-between` leaves a
               * *wrapped* single-item line at `flex-start`, which would
               * left-align the date under a long summary on desktop; with
               * the auto margin it reads flush right whether it shares the
               * summary's line or wraps below it.
               *
               * `rowGap` overrides `Group`'s own `md` gap on the wrap axis
               * only (an inline longhand beats the class's `gap` shorthand).
               * Without it a wrapped timestamp sat 16px under its summary
               * while the badge row below sat 4px under the timestamp, so
               * the date read as a label on the badges rather than on the
               * incident it belongs to. The horizontal `md` gap is
               * deliberately left alone.
               *
               * `overflowWrap: 'anywhere'` (inherited, so it reaches the
               * `TextLink` anchor, which takes no style of its own) is what
               * the removed `ScrollArea`'s `overflow: hidden` used to
               * provide by accident: a Knowledgebase summary can contain an
               * unbroken token longer than a 360px screen (a URL, a Welsh
               * station name), and a flex item's `min-width: auto` floor is
               * its longest word -- so without this the row would push the
               * whole page sideways. `anywhere` rather than `break-word`
               * precisely because it DOES lower that intrinsic minimum,
               * which is the whole point here; same choice, same reason, as
               * `.journeyProgressLabel` in `app/globals.css`. */}
              <Group style={{ rowGap: 2, overflowWrap: 'anywhere' }}>
                <TextLink href={`/incidents/${encodeURIComponent(row.incidentId)}`} underline="always">
                  {row.summary}
                </TextLink>
                <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap', marginInlineStart: 'auto' }}>
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
                {/* `?? []` is not defensive padding for its own sake: during
                  * a rolling deploy this bundle can be served against an api
                  * that predates `affectedLines`, and an unguarded `.map`
                  * would take the whole results list down rather than just
                  * omit the badges. Capped at MAX_LINE_BADGES because an
                  * operator-wide incident on a large TOC genuinely matches a
                  * dozen-plus catalogue lines. */}
                {(row.affectedLines ?? []).slice(0, MAX_LINE_BADGES).map((id) => (
                  <Badge key={id} variant="outline" color="blue" title="Affected line">
                    {lineNamesById.get(id) ?? id}
                  </Badge>
                ))}
                {(row.affectedLines ?? []).length > MAX_LINE_BADGES && (
                  <Badge
                    variant="outline"
                    color="blue"
                    /* The names themselves, not a generic label: collapsing
                     * must hide them from the layout, not lose them. */
                    title={(row.affectedLines ?? [])
                      .slice(MAX_LINE_BADGES)
                      .map((id) => lineNamesById.get(id) ?? id)
                      .join(', ')}
                  >
                    {`+${(row.affectedLines ?? []).length - MAX_LINE_BADGES} more`}
                  </Badge>
                )}
                {row.affectedStations.map((crs) => (
                  <Badge key={crs} variant="outline" color="gray">
                    {crs}
                  </Badge>
                ))}
              </Group>
            </Stack>
          ))}
        </Stack>
        <LoadMoreControl
          hasMore={results.nextCursor !== null}
          loading={loadingMore}
          failed={results.loadMoreFailed}
          onLoadMore={handleLoadMore}
          endMessage="You've reached the end — no more incidents match these filters."
        />
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      {/* Every `clearable` field below carries an explicit
          `clearButtonProps` aria-label. Mantine's `clearable` renders an
          `InputClearButton` with no accessible name at all, so axe's
          `button-name` fires (critical) the moment a field holds a value --
          which the two date fields do on first paint, because `applyPreset`
          seeds them. Same fix and same wording shape as
          `app/lines/AllLinesTable.tsx`'s own `clearButtonProps`, the one
          place in this app that already got this right.

          `className: 'iconHitArea24'` (globals.css) on all four: an
          `InputClearButton` is a `CloseButton` at its default `size="sm"`
          (22px), a hair under the 24px WCAG target-size floor -- review
          §2.10 measured the date-clear "×" at ~20px. Padded uniformly
          across all four clearable fields on this form rather than only
          the two the review named, so the row doesn't read as some clear
          buttons fixed and others not. */}
      <MultiSelect
        label="Operator (optional)"
        placeholder="Any operator"
        description="Matches an incident whose operators overlap any of these -- not 'scoped to exactly this operator.'"
        data={tocs.map((toc) => ({ value: toc.code, label: `${toc.code} — ${toc.name}` }))}
        value={operators}
        onChange={setOperators}
        searchable
        clearable
        clearButtonProps={{ 'aria-label': 'Clear operator filter', className: 'iconHitArea24' }}
      />
      <Select
        label="Line (optional)"
        placeholder="Any line"
        description="Incidents attributed to this line by the same matcher that drives its live status page. Incidents archived before this filter was fixed appear here only after a one-off reprocessing pass."
        data={catalogueLines.map((line) => ({ value: line.id, label: line.name }))}
        value={lineId}
        onChange={setLineId}
        searchable
        clearable
        clearButtonProps={{ 'aria-label': 'Clear line filter', className: 'iconHitArea24' }}
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
          clearButtonProps={{ 'aria-label': 'Clear the from date', className: 'iconHitArea24' }}
        />
        <DatePickerInput
          label="To (optional)"
          value={toDate}
          onChange={(value) => {
            setToDate(value);
            setPreset(null);
          }}
          clearable
          clearButtonProps={{ 'aria-label': 'Clear the to date', className: 'iconHitArea24' }}
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
