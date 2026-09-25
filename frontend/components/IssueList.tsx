'use client';

import { useId, useMemo, useState } from 'react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Badge,
  Button,
  Chip,
  ChipGroup,
  Collapse,
  Group,
  SegmentedControl,
  Stack,
  Text,
  Tooltip,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { StatusBadge } from './StatusBadge';
import { DisruptionDetail } from './DisruptionDetail';
import type { LineStatus } from '@/lib/types';
import { statusKey, type IssueItem } from '@/lib/stationIssues';
import { bucketFor, governingPeriod, periodIsActive, type IssueBucket } from '@/lib/validity';
import { formatDate, formatDateTime } from '@/lib/dateFormat';
import { isGoodSeverity } from '@/lib/severity';
import { impactTypeLabel } from '@/lib/impactType';
import { coverageProvenanceNote } from '@/lib/sampleStats';

type ActiveFilter = 'all' | IssueBucket;

/** Selects the noun used in the empty-state copy below. The line detail
 * page renders one `IssueList` per line, so 'line' is the sensible default
 * that lets that call site pass nothing; the station page renders a single
 * merged list deduped across every line through the station, so it must
 * pass 'station' explicitly or the empty states would misleadingly talk
 * about "this line" on a page that was never about just one. */
export type IssueListSubject = 'line' | 'station';

const BUCKET_SORT_RANK: Record<IssueBucket, number> = { active: 0, upcoming: 1, ended: 2 };

/** Review §2.14: chip-filter chrome (two rows of pills) competing for
 * attention with a report that's short enough to just read outright is the
 * imbalance the review names. A short report is exactly the case where
 * "narrow the list" isn't a real need yet, so at or below this many issues
 * the chips are tucked behind a "Filter" disclosure instead of always
 * shown. The All/Active/Upcoming `SegmentedControl` is NOT part of this --
 * it carries live counts and is useful standalone even on a one-issue
 * report. */
const FILTER_DISCLOSURE_MAX_ISSUES = 3;

const DATA_QUALITY_LABELS: Record<LineStatus['dataQuality'], string> = {
  knowledgebase: 'Knowledgebase',
  'ldbws-inferred': 'LDBWS-inferred',
  'trust-inferred': 'Trust-inferred',
  planned: 'Planned',
  tfl: 'TfL',
};

function formatValiditySummary(status: LineStatus, now: number): string {
  const period = governingPeriod(status, now);
  if (!period) return '';
  if (periodIsActive(period, now)) return 'Now';
  const from = formatDate(period.fromDate);
  return period.toDate ? `${from} – ${formatDate(period.toDate)}` : `From ${from}`;
}

function formatFullValidity(status: LineStatus, now: number): string {
  const period = governingPeriod(status, now);
  if (!period) return '';
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}

function pluraliseIssues(count: number): string {
  return count === 1 ? '1 issue' : `${count} issues`;
}

/** The empty state has to answer "why is this blank?", and on first load the
 * honest answer is rarely "your filters" — nobody has set one yet. Four
 * distinct situations, in order of how much the user can do about them:
 * nothing on the line at all; the chips excluded everything; the selected
 * tab is empty while another tab has content; and the same but with chips
 * genuinely narrowing (`chipsNarrowing`), which is the only case where
 * blaming filters is fair.
 *
 * `tab` is only ever 'active', 'upcoming' or 'ended' in the final branch:
 * the "All" tab shows the whole chip-filtered pool, so it cannot be empty
 * unless `pool` is 0, which the branch above already caught. */
function emptyStateMessage({
  total,
  pool,
  tab,
  activeCount,
  upcomingCount,
  endedCount,
  chipsNarrowing,
  subject,
}: {
  total: number;
  pool: number;
  tab: ActiveFilter;
  activeCount: number;
  upcomingCount: number;
  endedCount: number;
  chipsNarrowing: boolean;
  subject: IssueListSubject;
}): string {
  if (total === 0) return `No issues reported on this ${subject}.`;
  if (pool === 0) {
    return `No issues match the selected severity and source filters. Clear a filter to see the other ${pluraliseIssues(total)}.`;
  }

  const lead =
    tab === 'active'
      ? chipsNarrowing
        ? 'No active issues match the selected filters.'
        : `Nothing is affecting this ${subject} right now.`
      : tab === 'upcoming'
        ? chipsNarrowing
          ? 'No upcoming issues match the selected filters.'
          : `No issues are scheduled for later on this ${subject}.`
        : chipsNarrowing
          ? 'No ended issues match the selected filters.'
          : `Nothing on this ${subject} has finished recently.`;

  // Name the tab that actually holds something, so the user has somewhere to
  // go; "All" is the catch-all when every sibling tab is empty too.
  const siblings = [
    { label: 'Active', count: activeCount, value: 'active' as const },
    { label: 'Upcoming', count: upcomingCount, value: 'upcoming' as const },
    { label: 'Ended', count: endedCount, value: 'ended' as const },
  ].filter((candidate) => candidate.value !== tab);
  const sibling = siblings.find((candidate) => candidate.count > 0);
  const target = sibling ?? { label: 'All', count: pool };
  return `${lead} ${pluraliseIssues(target.count)} ${target.count === 1 ? 'is' : 'are'} listed under ${target.label}.`;
}

/** Two rows of identical grey pills gave no clue that they were controls,
 * which row meant what, or whether grey was "off" or "everything on". The
 * row label carries both the facet name and its state: an empty selection
 * filters nothing, which is not the same as filtering to nothing. */
function chipRowLabel(facet: string, selected: number): string {
  return selected === 0 ? `${facet} — showing all` : `${facet} — ${selected} selected`;
}

export function IssueList({
  items,
  now,
  subject = 'line',
}: {
  items: IssueItem[];
  now: number;
  subject?: IssueListSubject;
}) {
  const severityLabelId = useId();
  const sourceLabelId = useId();
  const statuses = useMemo(() => items.map((item) => item.status), [items]);
  const linesByStatus = useMemo(
    () => new Map(items.map((item) => [item.status, item.lines ?? []])),
    [items],
  );
  const severityOptions = Array.from(new Set(statuses.map((status) => status.statusSeverityDescription)));
  // Review §2.14: only offer a Source chip for a source actually present in
  // THIS loaded report, rather than every source `DATA_QUALITY_LABELS`
  // knows about network-wide -- a two-source report showing all five
  // possible chips implied narrowing that was never actually available.
  // `DATA_QUALITY_LABELS`' own key order is preserved (filtering, not
  // rebuilding, its entries) so this doesn't reshuffle the row.
  const presentSources = new Set(statuses.map((status) => status.dataQuality));
  const sourceOptions = Object.entries(DATA_QUALITY_LABELS).filter(([value]) =>
    presentSources.has(value as LineStatus['dataQuality']),
  );
  const [severityFilter, setSeverityFilter] = useState<string[]>([]);
  const [sourceFilter, setSourceFilter] = useState<string[]>([]);
  // Review §2.14: collapsed behind a disclosure on a short report -- see
  // `FILTER_DISCLOSURE_MAX_ISSUES`'s own comment. Always called (never
  // conditionally) for the same Rules-of-Hooks reason every other hook in
  // this component sits above the `allGood` early return below: the report
  // length that decides whether the disclosure is even offered can itself
  // change between renders of the same mounted instance.
  const [filtersOpened, { toggle: toggleFilters }] = useDisclosure(false);

  // `now` is stamped once by the Server Component page and passed in,
  // rather than read from `Date.now()` here. That keeps the server-rendered
  // markup and the client's pre-hydration render byte-identical — the same
  // constraint `LastUpdated` and `ThemeToggle` document — while still
  // letting the buckets depend on real dates. `AutoRefresh`'s 30s
  // `router.refresh()` re-stamps it, so it does not go stale.
  const buckets = useMemo(
    () => new Map(statuses.map((status) => [status, bucketFor(status, now)])),
    [statuses, now],
  );

  // Landing tab, decided once on mount (lazy initialiser) rather than
  // derived on every render. "Active" is the right place to open on a line
  // with live disruption, but a line whose issues are all planned/future
  // would open on an empty Active tab while the badge, summary sentence and
  // tab counts all say something is happening — which reads as a bug.
  // "All" is the fallback rather than "Upcoming" because it is the only tab
  // guaranteed to hold everything `statuses` has (an issue can be neither
  // active nor upcoming — a period that started in the past and has already
  // ended — and Upcoming would hide exactly those).
  //
  // Deliberately keyed off `statuses`, not `chipFiltered`, and never
  // recomputed: re-deriving would move the tab under the user the moment a
  // chip toggle happened to empty the current tab.
  const [activeFilter, setActiveFilter] = useState<ActiveFilter>(() =>
    statuses.some((status) => bucketFor(status, now) === 'active') ? 'active' : 'all',
  );

  // A line with nothing wrong doesn't need a filter UI. The old output was
  // a "GOOD SERVICE" header badge, two rows of filter chips, a three-tab
  // strip, and one expandable row whose entire content was a second
  // "GOOD SERVICE" badge and the words "Good Service" — three statements of
  // the same fact plus controls for narrowing a list of one non-issue.
  // Only when *every* status is Good Service: a line carrying both a good
  // service reading and a real disruption still needs the full list.
  //
  // This has to sit after every hook call in the component (useId/useMemo/
  // useState above), not "immediately after the statuses/linesByStatus
  // memos" as originally sketched — Task 10's `severityFilter`/
  // `sourceFilter`/`activeFilter` state and the `buckets` memo all sit
  // between those memos and here now, and an early return before them
  // would call a different number of hooks depending on `allGood`, which
  // breaks React's Rules of Hooks. Confirmed by reproduction: rerendering
  // the same mounted instance across an all-good/mixed transition (which
  // `router.refresh()` can do without remounting, since Client Components
  // preserve state across a Server Component refresh) throws "Rendered
  // fewer/more hooks than expected." Placing the check after the last hook
  // call and before the first plain (non-hook) derived value keeps every
  // hook unconditional while still skipping all of the chrome below.
  const allGood = statuses.length > 0 && statuses.every((status) => isGoodSeverity(status.statusSeverity));
  if (allGood) {
    return <Text c="dimmed">Good service — no issues reported on this {subject}.</Text>;
  }

  // Severity/source chips narrow the pool every tab counts from, but not
  // the active/upcoming/ended tab itself — so switching tabs doesn't change
  // the other tabs' counts, matching a standard faceted-filter count
  // pattern.
  const chipFiltered = statuses.filter((status) => {
    if (severityFilter.length > 0 && !severityFilter.includes(status.statusSeverityDescription)) return false;
    if (sourceFilter.length > 0 && !sourceFilter.includes(status.dataQuality)) return false;
    return true;
  });
  const activeCount = chipFiltered.filter((s) => buckets.get(s) === 'active').length;
  const upcomingCount = chipFiltered.filter((s) => buckets.get(s) === 'upcoming').length;
  const endedCount = chipFiltered.filter((s) => buckets.get(s) === 'ended').length;

  function sortRank(status: LineStatus): number {
    return BUCKET_SORT_RANK[buckets.get(status) ?? 'ended'];
  }

  function earliestFromDate(status: LineStatus): number {
    const period = governingPeriod(status, now);
    return period ? Date.parse(period.fromDate) : Infinity;
  }

  // Active first, then upcoming, then ended — matches the tab order.
  // Within a group, earliest `fromDate` first ("what's happening/starting
  // soonest").
  function compareByUrgency(a: LineStatus, b: LineStatus): number {
    const rankDiff = sortRank(a) - sortRank(b);
    if (rankDiff !== 0) return rankDiff;
    // `Infinity - Infinity` is `NaN`, an invalid Array.sort comparator
    // result — reached when two statuses in the same group both lack a
    // governing period (only possible when `validityPeriods` is empty,
    // which `bucketFor` treats as active, so this is a defensive fallback
    // rather than a reachable case today).
    return earliestFromDate(a) - earliestFromDate(b) || 0;
  }

  const filtered = chipFiltered
    .filter((status) => activeFilter === 'all' || buckets.get(status) === activeFilter)
    .sort(compareByUrgency);

  // Extracted so it can render either always-visible (a report large enough
  // that narrowing it is plausibly useful on first paint) or tucked behind
  // the "Filter" disclosure below (a short report, where it's dead chrome
  // until asked for) -- same chip rows, same handlers, either way.
  const chipRows = (
    <Stack gap="xs">
      <Stack gap={4}>
        <Text id={severityLabelId} size="xs" fw={600} c="dimmed">
          {chipRowLabel('Severity', severityFilter.length)}
        </Text>
        <ChipGroup multiple value={severityFilter} onChange={setSeverityFilter}>
          <Group gap="xs" role="group" aria-labelledby={severityLabelId}>
            {severityOptions.map((option) => (
              // `filled` vs `outline` is the whole point: an unselected chip
              // reads as an empty control you can press, a selected one as a
              // solid, obviously-on state — indistinguishable before.
              <Chip
                key={option}
                value={option}
                size="xs"
                variant={severityFilter.includes(option) ? 'filled' : 'outline'}
              >
                {option}
              </Chip>
            ))}
          </Group>
        </ChipGroup>
      </Stack>
      <Stack gap={4}>
        <Text id={sourceLabelId} size="xs" fw={600} c="dimmed">
          {chipRowLabel('Source', sourceFilter.length)}
        </Text>
        <ChipGroup multiple value={sourceFilter} onChange={setSourceFilter}>
          <Group gap="xs" role="group" aria-labelledby={sourceLabelId}>
            {sourceOptions.map(([value, label]) => (
              <Chip
                key={value}
                value={value}
                size="xs"
                variant={sourceFilter.includes(value) ? 'filled' : 'outline'}
              >
                {label}
              </Chip>
            ))}
          </Group>
        </ChipGroup>
      </Stack>
    </Stack>
  );
  const showFilterDisclosure = statuses.length <= FILTER_DISCLOSURE_MAX_ISSUES;
  const filterSelectionCount = severityFilter.length + sourceFilter.length;

  return (
    <Stack gap="md">
      <Stack gap="xs">
        {showFilterDisclosure ? (
          <Stack gap={4}>
            <Button
              variant="subtle"
              size="compact-xs"
              onClick={toggleFilters}
              aria-expanded={filtersOpened}
            >
              {filtersOpened
                ? 'Hide filters'
                : filterSelectionCount > 0
                  ? `Filter (${filterSelectionCount} active)`
                  : 'Filter'}
            </Button>
            {/* `keepMounted={false}`: same rationale as the `Accordion`
                below -- Mantine's default keeps collapsed content in the
                DOM (via React's Activity API), merely `inert`/hidden from
                view, which `screen.queryByText` still finds. Unmounting
                outright means "collapsed" also means the chip filters
                genuinely aren't reachable, not just invisible.
                `transitionDuration={0}`: a two-chip-row toggle doesn't need
                a slide animation, and it keeps `keepMounted={false}`'s
                mount/unmount synchronous with the click that drives it
                (Mantine's `useCollapse` otherwise defers the mount across a
                `requestAnimationFrame` pair plus a real `transitionend`
                event -- neither of which jsdom ever fires on its own). */}
            <Collapse expanded={filtersOpened} keepMounted={false} transitionDuration={0}>
              {chipRows}
            </Collapse>
          </Stack>
        ) : (
          chipRows
        )}
        <SegmentedControl
          value={activeFilter}
          onChange={(value) => setActiveFilter(value as ActiveFilter)}
          data={[
            { label: `All (${chipFiltered.length})`, value: 'all' },
            { label: `Active (${activeCount})`, value: 'active' },
            { label: `Upcoming (${upcomingCount})`, value: 'upcoming' },
            // Only offered when something is actually in it. Three buckets
            // could never make the counts add up — an issue whose window
            // has closed is neither active nor upcoming — and "3 issues,
            // 0 active, 0 upcoming" is exactly the nonsense this fixes.
            ...(endedCount > 0 ? [{ label: `Ended (${endedCount})`, value: 'ended' }] : []),
          ]}
        />
      </Stack>

      {filtered.length === 0 && (
        <Text c="dimmed">
          {emptyStateMessage({
            total: statuses.length,
            pool: chipFiltered.length,
            tab: activeFilter,
            activeCount,
            upcomingCount,
            endedCount,
            chipsNarrowing: chipFiltered.length < statuses.length,
            subject,
          })}
        </Text>
      )}

      {/*
        keepMounted={false}: Mantine v9's AccordionPanel defaults to keeping
        collapsed panel content mounted (via React's Activity API) purely
        hidden from view. That's invisible to sighted users but still present
        in the DOM, so `screen.queryByText` still finds it — unmount collapsed
        panels outright so "collapsed by default" also means "not rendered".
      */}
      <Accordion multiple keepMounted={false}>
        {/*
          Bug: this used to be `key={i} value={String(i)}` -- the ARRAY
          INDEX into `filtered`, not a property of the issue itself. Mantine's
          Accordion tracks which panels are expanded by `value`, so once a
          user expanded (say) index 2 and then a chip/tab filter change
          re-sorted or shortened `filtered`, index 2 could silently become a
          *different* issue -- the panel stayed "expanded" but now showed the
          wrong incident's detail underneath the same position. `statusKey`
          (`lib/stationIssues.ts`) is this codebase's own established stable
          identity for a `LineStatus` (already used to dedupe issues across
          reports), so it's keyed/valued on that instead -- tied to the
          issue's own severity/reason/validity-window, not its position in
          this render's array.
        */}
        {filtered.map((status) => (
          <AccordionItem key={statusKey(status)} value={statusKey(status)}>
            <AccordionControl>
              {/*
                The badges are the row's classification and provenance, so
                they hold their width; the reason is the only element that
                gives way. All of that — and the two-line stack below `sm` —
                lives in `app/globals.css`'s `.issueRow*` rules rather than
                in Mantine `Group` props, because a breakpoint cannot be
                expressed as a style object and the previous inline
                `flexShrink`/`minWidth`/`wrap="nowrap"` values would have
                outranked any media query that tried.
              */}
              <div className="issueRow">
                <div className="issueRow__main">
                  <div className="issueRow__badge">
                    <StatusBadge severity={status.statusSeverity} />
                  </div>
                  <Text size="sm" className="issueRow__reason">
                    {status.reason}
                  </Text>
                </div>
                <div className="issueRow__meta">
                  <Text size="xs" c="dimmed">
                    {formatValiditySummary(status, now)}
                  </Text>
                  {impactTypeLabel(status.disruption?.impactType) && (
                    <Badge variant="light" size="sm" color="orange">
                      {impactTypeLabel(status.disruption?.impactType)}
                    </Badge>
                  )}
                  {(linesByStatus.get(status) ?? []).length > 1 && (
                    <Badge variant="outline" size="sm" color="gray">
                      {linesByStatus.get(status)!.length} lines
                    </Badge>
                  )}
                  {/*
                    Explicit gray: without a `color`, Mantine falls back to
                    theme.primaryColor (grape), making this read as branded
                    or interactive. It's provenance, not brand — gray is
                    already how `informational` severity is treated in
                    lib/severity.ts's GROUP_COLOR.
                  */}
                  {/*
                    Decision 2: where the confident third copy branch plugs
                    into DataQuality badge rendering. Per-status attachment
                    already makes this a per-status decision, not a
                    per-line one -- mirrors this file's own existing
                    mixed-state correctness (Decision 3's "IssueList.tsx...
                    already correct... no design work needed beyond that"),
                    so a line with one full-coverage-confirmed status and
                    one still-sampled status shows the confident tooltip on
                    only the former. `undefined` (no tooltip) for every
                    status today, since nothing carries `fullCoverageStats`
                    yet -- forward-looking scaffolding.
                  */}
                  <Tooltip label={coverageProvenanceNote(status) ?? ''} disabled={!coverageProvenanceNote(status)}>
                    <Badge variant="outline" size="sm" color="gray">
                      {DATA_QUALITY_LABELS[status.dataQuality]}
                    </Badge>
                  </Tooltip>
                </div>
              </div>
            </AccordionControl>
            <AccordionPanel>
              <Stack gap="xs">
                {(linesByStatus.get(status) ?? []).length > 1 && (
                  <Text size="sm" c="dimmed">
                    Affects: {linesByStatus.get(status)!.map((line) => line.name).join(', ')}
                  </Text>
                )}
                <Text size="sm" c="dimmed">
                  Valid: {formatFullValidity(status, now)}
                </Text>
                {status.disruption ? (
                  <DisruptionDetail disruption={status.disruption} />
                ) : (
                  <Text c="dimmed" size="sm">
                    No further detail available.
                  </Text>
                )}
              </Stack>
            </AccordionPanel>
          </AccordionItem>
        ))}
      </Accordion>
    </Stack>
  );
}
