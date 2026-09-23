'use client';

import { useEffect, useId, useState, type FormEvent } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import {
  Alert,
  Badge,
  Box,
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

/** Parses `priority_min`/`priority_max`'s raw URL string into the numeric
 * value `priorityMin`/`priorityMax` state expects, folding "absent",
 * "present but blank" (a literal `?priority_min=`) and "not a real, finite
 * number at all" into the same `''` (blank) result `NumberInput` already
 * treats as "no filter" -- same "malformed means absent" posture
 * `app/track/page.tsx`'s `validTimeParam`/`ticketIdParam` already take
 * elsewhere in this codebase, just applied inside the component rather than
 * the page since these two params aren't pre-validated by their caller.
 * `Number.isFinite` rather than `!Number.isNaN`: `Number('')` is `0` (not
 * `NaN`), which the blank-string check above already intercepts, but
 * `Number('Infinity')` is a real, finite-looking `NaN`-free number the
 * NaN-only check would have let straight through as a filter value.
 * Never throws on a garbage value in the URL. */
function parsePriorityParam(value: string | undefined): number | '' {
  if (value === undefined || value.trim() === '') return '';
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : '';
}

/** A `nothingFoundMessage` node, not a plain string: Mantine's own
 * `Combobox.Empty` (what `nothingFoundMessage` renders into,
 * `OptionsDropdown.tsx` in `@mantine/core`) is an unstyled `<Box>` with no
 * ARIA role at all, so a plain string leaves the `role="listbox"` wrapper
 * with a child that is neither an `option` nor a `group` -- axe's
 * `aria-required-children` (critical) fires on exactly that, confirmed live
 * against this form's own Operator field once the TOC catalogue (or a
 * search) comes back empty. `Combobox.Empty` forwards unrecognised props
 * straight onto that `<Box>` (it's a thin wrapper, no prop allowlist), so
 * `role="option"` + `aria-disabled` here is enough to make the listbox
 * structurally valid without touching Mantine internals -- `aria-disabled`
 * (state, not `disabled`) keeps it out of the tab order's expectations
 * without hiding it, matching how a real disabled option would read. */
function noOptionsFound(label: string) {
  return (
    <Box role="option" aria-disabled="true">
      {label}
    </Box>
  );
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
  initialPeriod,
  initialPlanned,
  initialCleared,
  initialPriorityMin,
  initialPriorityMax,
}: {
  lines: LineSummary[];
  tocs: Suggestion[];
  initialOperator?: string;
  initialLine?: string;
  initialFrom?: string;
  initialTo?: string;
  initialPeriod?: string;
  initialPlanned?: string;
  initialCleared?: string;
  initialPriorityMin?: string;
  initialPriorityMax?: string;
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
    initialPeriod === 'all' ? null : initialFrom ? initialFrom.slice(0, 10) : calendarDaysAgo(30),
  );
  const [toDate, setToDate] = useState<string | null>(
    initialPeriod === 'all' ? null : initialTo ? initialTo.slice(0, 10) : null,
  );
  const [preset, setPreset] = useState<DatePreset | null>(
    initialPeriod === 'all' ? 'all' : initialFrom ? null : '30d',
  );
  const [plannedFilter, setPlannedFilter] = useState<'all' | 'planned' | 'realtime'>(
    initialPlanned === 'true' ? 'planned' : initialPlanned === 'false' ? 'realtime' : 'all',
  );
  const [clearedFilter, setClearedFilter] = useState<'all' | 'active' | 'cleared'>(
    initialCleared === 'false' ? 'active' : initialCleared === 'true' ? 'cleared' : 'all',
  );
  const [priorityMin, setPriorityMin] = useState<number | ''>(parsePriorityParam(initialPriorityMin));
  const [priorityMax, setPriorityMax] = useState<number | ''>(parsePriorityParam(initialPriorityMax));
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

  /** Review §2.13: this used to be a row of filled/light preset buttons that
   * applied immediately, sitting above a pair of always-visible From/To date
   * pickers that a hand-edit could silently disagree with (no preset stayed
   * highlighted once either field was touched). One `SegmentedControl`
   * fixes both: "Custom…" is always the correct, real answer once a date has
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
      const body: IncidentSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor, query, loadMoreFailed: false });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleSubmit(event: FormEvent) {
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
   * Also restores `plannedFilter`/`clearedFilter`/`priorityMin`/
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
    if (query) void runSearch(query);
    // Intentionally empty: this is a mount-only effect, not one that
    // tracks the filter state it reads -- see the comment above.
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
                  <Badge key={crs} variant="outline" color="gray" title="Affected station">
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
            description="Matches an incident whose operators overlap any of these -- not 'scoped to exactly this operator.'"
            data={tocs.map((toc) => ({ value: toc.code, label: `${toc.code} — ${toc.name}` }))}
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
            description="Incidents attributed to this line, using the same rules as its live status page. Incidents archived before this filter existed were only linked up to their line once, in a one-off catch-up run."
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
                { label: 'Custom…', value: 'custom' },
              ]}
            />
          </Stack>
          {preset === null && (
            <Group align="end">
              <DatePickerInput
                label="From (optional)"
                placeholder="Any"
                value={fromDate}
                onChange={setFromDate}
                clearable
                clearButtonProps={{ 'aria-label': 'Clear the from date', className: 'iconHitArea24' }}
              />
              <DatePickerInput
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
              onChange={(value) => setPlannedFilter(value as 'all' | 'planned' | 'realtime')}
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
              value={clearedFilter}
              onChange={(value) => setClearedFilter(value as 'all' | 'active' | 'cleared')}
              data={[
                { label: 'All', value: 'all' },
                { label: 'Active', value: 'active' },
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
                &quot;major&quot;/&quot;minor&quot; meaning — shown as-is, not a severity scale.
              </>
            }
            error={!priorityValid ? 'Minimum must not exceed maximum' : null}
          >
            <Group grow align="flex-start">
              <NumberInput
                label="Minimum"
                value={priorityMin}
                onChange={(value) => setPriorityMin(typeof value === 'number' ? value : '')}
              />
              <NumberInput
                label="Maximum"
                value={priorityMax}
                onChange={(value) => setPriorityMax(typeof value === 'number' ? value : '')}
              />
            </Group>
          </InputWrapper>
          <Group>
            <Button type="submit" disabled={!priorityValid || searching}>
              {searching ? 'Searching…' : 'Search'}
            </Button>
          </Group>
        </Stack>
      </Grid.Col>
      <Grid.Col span={{ base: 12, md: 7 }}>
        <Stack gap="xs" mih={72}>
          {resultsContent()}
        </Stack>
      </Grid.Col>
    </Grid>
  );
}
