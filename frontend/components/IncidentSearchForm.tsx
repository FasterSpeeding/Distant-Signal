'use client';

import { useEffect, useId, useState, type SubmitEvent } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import {
  Button,
  Grid,
  Group,
  InputWrapper,
  MultiSelect,
  NumberInput,
  SegmentedControl,
  Select,
  Stack,
  Text,
} from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { londonCalendarDay, londonDayEndIso, londonDayStartIso } from '@/lib/londonWallClock';
import { IncidentResults } from './IncidentResults';
import { noOptionsFound } from './noOptionsFound';
import type { IncidentSearchResponse, LineSummary, Suggestion } from '@/lib/types';
import { ButtonSpinner } from '@/components/LoadingText';
import {
  calendarDaysAgo,
  initialStateFilter,
  parsePriorityParam,
  type DatePreset,
  type IncidentResults as IncidentResultsState,
  type StateFilter,
} from '@/lib/incidentSearch';

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
  initialPeriod,
  initialPlanned,
  initialState,
  initialCleared,
  initialPriorityMin,
  initialPriorityMax,
}: {
  lines: LineSummary[];
  tocs: Suggestion[];
  initialOperator?: string | undefined;
  initialLine?: string | undefined;
  initialFrom?: string | undefined;
  initialTo?: string | undefined;
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
  const router = useRouter();
  const pathname = usePathname();
  const periodLabelId = useId();
  const typeLabelId = useId();
  const statusLabelId = useId();
  const catalogueLines = lines.filter((line) => line.source === 'catalogue');
  /** Line id -> display name, so a result row's `affectedLines` renders as
   * "Elizabeth line" rather than "elizabeth-line". An id with no entry (a
   * line retired from the catalogue since the incident was ingested) falls
   * back to the raw id rather than disappearing. */
  const lineNamesById = new Map(catalogueLines.map((line) => [line.id, line.name]));
  const tocNamesByCode = new Map(tocs.map((toc) => [toc.code, toc.name]));

  const [operators, setOperators] = useState<string[]>(
    initialOperator ? initialOperator.split(',').filter(Boolean) : [],
  );
  const [lineId, setLineId] = useState<string | null>(initialLine || null);
  /** `initialPeriod === 'all'` takes priority over `initialFrom`/
   * `initialTo` for exactly the same reason `applyPreset('all')` clears
   * both when a caller picks "All time" live: restoring "All time" means
   * restoring NO lower/upper bound, full stop, matching what a real
   * `?period=all` URL only ever gets written alongside (see `handleSubmit`'s
   * own comment on why `period` exists at all). A plain, unfiltered first
   * visit -- no `initialFrom`, no `initialPeriod` -- must still fall
   * through to the existing 30-day floor exactly as before this prop
   * existed. */
  const [fromDate, setFromDate] = useState<string | null>(
    initialPeriod === 'all' ? null : initialFrom ? londonCalendarDay(initialFrom) : calendarDaysAgo(30),
  );
  const [toDate, setToDate] = useState<string | null>(
    initialPeriod === 'all' ? null : initialTo ? londonCalendarDay(initialTo) : null,
  );
  const [preset, setPreset] = useState<DatePreset | null>(initialPeriod === 'all' ? 'all' : initialFrom ? null : '30d');
  const [plannedFilter, setPlannedFilter] = useState<'all' | 'planned' | 'realtime'>(
    initialPlanned === 'true' ? 'planned' : initialPlanned === 'false' ? 'realtime' : 'all',
  );
  const [stateFilter, setStateFilter] = useState<StateFilter>(initialStateFilter(initialState, initialCleared));
  const [priorityMin, setPriorityMin] = useState<number | ''>(parsePriorityParam(initialPriorityMin));
  const [priorityMax, setPriorityMax] = useState<number | ''>(parsePriorityParam(initialPriorityMax));
  const [results, setResults] = useState<IncidentResultsState>(null);
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

  /** Review §2.13: this used to be a row of filled/light preset buttons that
   * applied immediately, sitting above a pair of always-visible From/To date
   * pickers that a hand-edit could silently disagree with (no preset stayed
   * highlighted once either field was touched). One `SegmentedControl`
   * fixes both: "Custom dates" is always the correct, real answer once a date has
   * been hand-edited or is being edited at all, and the two picker fields
   * below only render while it's selected. */
  function handlePeriodChange(next: string) {
    if (next === 'custom') {
      // No date reset -- this only reveals the picker fields for whatever
      // from/to they already held (e.g. seeded by an initial `?from=`, or
      // left over from a previous preset).
      setPreset(null);
      return;
    }
    applyPreset(next as DatePreset);
  }

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
  function searchParamsFor() {
    const params = new URLSearchParams();
    if (operators.length > 0) params.set('operator', operators.join(','));
    if (lineId) params.set('line', lineId);
    // `fromDate`/`toDate` are date-only (`YYYY-MM-DD`) picker values naming
    // LONDON calendar days (FE-5), like every other day grouping in this
    // app. The backend's `to` is an inclusive `first_seen_at <= to`
    // comparison, so it gets the last millisecond of that London day.
    if (fromDate) params.set('from', londonDayStartIso(fromDate));
    if (toDate) params.set('to', londonDayEndIso(toDate));
    if (plannedFilter === 'planned') params.set('planned', 'true');
    if (plannedFilter === 'realtime') params.set('planned', 'false');
    // `state`, not the legacy `cleared` boolean: `cleared=false` can only
    // say "not cleared", and "Ended" (no longer listed by the feed) is a
    // third state of its own.
    if (stateFilter !== 'all') params.set('state', stateFilter);
    if (priorityMin !== '') params.set('priority_min', String(priorityMin));
    if (priorityMax !== '') params.set('priority_max', String(priorityMax));
    return params;
  }

  /** The actual `GET /public/incidents` call, shared by the explicit Search
   * button (`handleSubmit`) and the auto-run-on-mount effect below, so a
   * search fired either way behaves identically -- same loading state, same
   * error handling, same `Results` shape installed on success. `query` is
   * captured by the CALLER, not rebuilt here -- mirroring `TrainSearchForm
   * .tsx`'s own capture of `date` inside its `Results` success variant.
   * `handleLoadMore` below reuses this same captured string for every
   * subsequent page of a result set rather than rebuilding it from live
   * filter state, so a filter changed after searching (but before "Load
   * more" is pressed) cannot silently mix into a page fetched with the
   * original cursor. */
  async function runSearch(query: string) {
    setSearching(true);
    try {
      const response = await fetch(`/api/incidents?${query}`);
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body = (await response.json()) as IncidentSearchResponse;
      setResults({ rows: body.results, nextCursor: body.nextCursor, query, loadMoreFailed: false });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    if (!priorityValid || searching) return;
    // Keep this /incidents history entry's URL in sync with the last search
    // that actually ran, so following a result to
    // `/incidents/[incidentId]` and pressing Back can restore both the form
    // and the results, rather than re-delivering the stale URL /incidents
    // first loaded with (see
    // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
    // §3.1). `replace`, not `push`: this keeps /incidents a single history
    // entry whose URL stays current, not a new Back-button stop on every
    // search -- mirrors `TrainSearchForm.tsx`'s own identical use.
    // Deliberately NOT done from the mount effect below (§3.3) -- only an
    // explicit Search press writes to the URL. The same query string that
    // is what actually gets searched (`query`, handed to `runSearch`
    // unmodified) is not always exactly what gets written to the URL,
    // though -- see `urlParams` below.
    const query = searchParamsFor().toString();
    // Final whole-branch review fix round 1: `searchParamsFor()` omits
    // `from` entirely whenever `fromDate` is falsy, and that is true for
    // BOTH "no lower bound was ever set" (a plain first visit, before the
    // 30-day floor's own `useState` seeds it) and "All time" was explicitly
    // chosen (`applyPreset('all')` nulls `fromDate` on purpose) -- the two
    // are indistinguishable on the wire, which is exactly correct for the
    // API (it has no "explicitly no bound" wire value to send) but wrong
    // for the URL: restoring a URL with no `from` at all falls back to the
    // 30-day floor (see the `fromDate` `useState` above), silently turning
    // an "All time" search into a materially different, narrower one on
    // Back-navigation, with no signal anything changed. `period=all` is a
    // URL-only marker with no equivalent on the wire (`searchParamsFor()`
    // itself never emits it, and the backend never sees it) -- purely so
    // `initialPeriod` above can tell "All time was explicitly chosen" apart
    // from "no filter was ever set" the same way `preset` already can in
    // memory. Built as a COPY of `query` (`new URLSearchParams(query)`)
    // rather than folded into `searchParamsFor()` itself, precisely so it
    // stays absent from the string `runSearch`/`fetch` actually send.
    const urlParams = new URLSearchParams(query);
    if (!fromDate) urlParams.set('period', 'all');
    // A literal empty form (no filters, all pickers cleared) must not
    // leave a dangling `?` on the URL -- same idiom
    // `AllLinesTable.tsx`'s own `router.replace` already uses for the
    // identical "possibly-empty query string" case.
    router.replace(urlParams.size > 0 ? `${pathname}?${urlParams.toString()}` : pathname, {
      scroll: false,
    });
    await runSearch(query);
  }

  /** Review §3.3: the archive used to land on an empty "Press Search to
   * browse incidents" placeholder despite a default filter already being
   * applied -- the 30-day `fromDate` floor this component seeds on first
   * render (see the `useState` above) makes the initial filter set
   * non-empty on every load, so a first-time visitor's very first view was
   * a dead form, not a useful one. Runs exactly once, against whatever
   * filter set the component mounted with -- an empty dependency array,
   * deliberately NOT re-run on every later filter edit, which stays the
   * Search button's own job (`handleSubmit`). The `query` guard is mostly
   * defensive: today it is always non-empty because of the 30-day floor,
   * but a future change removing that default must not turn this into an
   * unasked-for "search everything" on every page load.
   *
   * Also restores `plannedFilter`/`stateFilter`/`priorityMin`/
   * `priorityMax` from the URL now, not just the default 30-day-floor
   * search this comment originally described: once those four fields'
   * own `initialX` props seed the `useState`s above, this same
   * `searchParamsFor()` call picks them up for free, with no logic change
   * needed here. That is what lets a Back-navigation after visiting
   * `/incidents/[incidentId]` restore the RESULTS too, not just the form
   * fields -- see
   * docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
   * §3.3.
   *
   * Known, accepted limitation (§3.4): if the visitor had pressed "Load
   * more" one or more times before leaving, only PAGE 1 of that result set
   * is reconstructed here -- `results.nextCursor` is server-issued opaque
   * pagination state with no representation in `searchParamsFor()`/the URL
   * at all, so there is nothing for this effect to resume from. This
   * re-runs the FIRST page of the same search, not a resume of exactly
   * where "Load more" had gotten to -- a correct, if smaller, restoration
   * rather than a wrong one, and not a bug to fix in this pass. */
  useEffect(() => {
    const query = searchParamsFor().toString();
    // eslint-disable-next-line react-hooks/set-state-in-effect -- mount-only restore of the URL's search (see above)
    if (query) void runSearch(query);
    // Intentionally empty: this is a mount-only effect, not one that
    // tracks the filter state it reads -- see the comment above.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount-only effect (see above)
  }, []);

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

  return (
    // Review §3.3: at 1440 the filter fields used to stretch across the
    // whole ~1100px `<main>` container, reading as a table's worth of
    // controls rather than a form -- and the archive's own ~700px-tall
    // form sat above every result, so a first-time visitor scrolled past
    // all of it before seeing a single incident. A two-column `Grid`
    // fixes both at once: filters keep to a `maw`-capped left column (so
    // they read as a form at any container width, not just once this grid
    // narrows them), and results run beside them rather than below a full
    // page of controls. Deliberately NOT reordered with `Grid.Col`'s own
    // `order` prop -- DOM order (filters, then results) stays the reading
    // order for keyboard/screen-reader users at every width, and on a
    // narrow viewport both columns collapse to `span=12` and stack in
    // that same order anyway, which is the existing, unchanged mobile
    // shape.
    <Grid gap="xl" component="form" onSubmit={handleSubmit}>
      <Grid.Col span={{ base: 12, md: 5 }}>
        <Stack gap="md" maw={720}>
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
            description="Incidents involving any of these operators."
            data={tocs.map((toc) => ({ value: toc.code, label: `${toc.code} · ${toc.name}` }))}
            value={operators}
            onChange={setOperators}
            searchable
            clearable
            // Mantine's `Combobox` hides the whole dropdown outright when it
            // has zero options and no `nothingFoundMessage` is set
            // (`hiddenWhenEmpty: !nothingFoundMessage`, OptionsDropdown.tsx)
            // -- not just "no rows", the `role="listbox"` element itself
            // never mounts, while the combobox input is left with
            // `aria-expanded="true"` pointing at nothing. That is reachable
            // whenever the TOC catalogue is empty (this environment's
            // fixture data) or a search narrows a real catalogue to zero
            // matches (production), so it is not a fixture-only edge case.
            nothingFoundMessage={noOptionsFound('No matching operators')}
            clearButtonProps={{ 'aria-label': 'Clear operator filter', className: 'iconHitArea24' }}
          />
          <Select
            label="Line (optional)"
            placeholder="Any line"
            description="Incidents on this line, matched the same way as its status page."
            data={catalogueLines.map((line) => ({ value: line.id, label: line.name }))}
            value={lineId}
            onChange={setLineId}
            searchable
            clearable
            // Same empty-dropdown gap as the Operator field above -- see its
            // comment.
            nothingFoundMessage={noOptionsFound('No matching lines')}
            clearButtonProps={{ 'aria-label': 'Clear line filter', className: 'iconHitArea24' }}
          />
          <Stack gap={4}>
            <Text id={periodLabelId} size="xs" fw={600} c="dimmed">
              Period
            </Text>
            <SegmentedControl
              aria-labelledby={periodLabelId}
              color="grape"
              // See `HistoryRangePicker`'s identical `autoContrast={false}`
              // comment: `SegmentedControl`'s active-label contrast decision
              // bypasses `lib/theme.ts`'s grape-filled `variantColorResolver`
              // pin and is scheme-blind in the same way gray/blue's filled
              // variant used to be, landing on black text at 4.33:1 against
              // the grape-7 background this app's light scheme actually
              // renders. Forcing white here matches the pin already applied
              // everywhere else grape paints a filled surface.
              autoContrast={false}
              value={preset ?? 'custom'}
              onChange={handlePeriodChange}
              data={[
                { label: '7 days', value: '7d' },
                { label: '30 days', value: '30d' },
                { label: '90 days', value: '90d' },
                { label: 'All time', value: 'all' },
                { label: 'Custom dates', value: 'custom' },
              ]}
            />
          </Stack>
          {preset === null && (
            <Group align="end">
              <DatePickerInput
                valueFormat="D MMM YYYY"
                label="From (optional)"
                placeholder="Any"
                value={fromDate}
                onChange={setFromDate}
                clearable
                clearButtonProps={{ 'aria-label': 'Clear the from date', className: 'iconHitArea24' }}
              />
              <DatePickerInput
                valueFormat="D MMM YYYY"
                label="To (optional)"
                placeholder="Any"
                value={toDate}
                onChange={setToDate}
                clearable
                clearButtonProps={{ 'aria-label': 'Clear the to date', className: 'iconHitArea24' }}
              />
            </Group>
          )}
          <Stack gap={4}>
            <Text id={typeLabelId} size="xs" fw={600} c="dimmed">
              Type
            </Text>
            <SegmentedControl
              aria-labelledby={typeLabelId}
              color="grape"
              // See the Period `SegmentedControl` above for why.
              autoContrast={false}
              value={plannedFilter}
              onChange={(value) => setPlannedFilter(value)}
              data={[
                { label: 'All', value: 'all' },
                { label: 'Planned work', value: 'planned' },
                { label: 'Real-time', value: 'realtime' },
              ]}
            />
          </Stack>
          <Stack gap={4}>
            <Text id={statusLabelId} size="xs" fw={600} c="dimmed">
              Status
            </Text>
            <SegmentedControl
              aria-labelledby={statusLabelId}
              color="grape"
              // See the Period `SegmentedControl` above for why.
              autoContrast={false}
              value={stateFilter}
              onChange={(value) => setStateFilter(value)}
              data={[
                { label: 'All', value: 'all' },
                { label: 'Active', value: 'active' },
                { label: 'Ended', value: 'ended' },
                { label: 'Cleared', value: 'cleared' },
              ]}
            />
          </Stack>
          {/* Review §2.14: the raw-feed caveat used to repeat three times --
              once in each NumberInput's own label, plus a third standalone
              footnote paragraph below both. Keeping priority visibly
              unprocessed and honestly labelled is still the right call (the
              feed genuinely has no documented "major"/"minor" meaning) --
              this only says so once, as this `Input.Wrapper`'s own
              description. */}
          <InputWrapper
            label="Priority range"
            description={
              <>
                Raw feed value from National Rail&apos;s own incident data with no documented
                &quot;major&quot;/&quot;minor&quot; meaning. Shown as-is, not a severity scale.
              </>
            }
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
          <Group>
            <Button
              type="submit"
              disabled={!priorityValid || searching}
              leftSection={searching ? <ButtonSpinner /> : undefined}
            >
              {searching ? 'Searching' : 'Search'}
            </Button>
          </Group>
        </Stack>
      </Grid.Col>
      <Grid.Col span={{ base: 12, md: 7 }}>
        <Stack gap="xs" mih={72}>
          <IncidentResults
            results={results}
            searching={searching}
            loadingMore={loadingMore}
            onLoadMore={() => void handleLoadMore()}
            tocNamesByCode={tocNamesByCode}
            lineNamesById={lineNamesById}
          />
        </Stack>
      </Grid.Col>
    </Grid>
  );
}
