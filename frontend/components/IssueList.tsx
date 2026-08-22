'use client';

import { useId, useState } from 'react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Badge,
  Chip,
  ChipGroup,
  Group,
  SegmentedControl,
  Stack,
  Text,
} from '@mantine/core';
import { StatusBadge } from './StatusBadge';
import { DisruptionDetail } from './DisruptionDetail';
import type { LineStatus } from '@/lib/types';

type ActiveFilter = 'all' | 'active' | 'upcoming';

const DATA_QUALITY_LABELS: Record<LineStatus['dataQuality'], string> = {
  knowledgebase: 'Knowledgebase',
  'ldbws-inferred': 'LDBWS-inferred',
  'trust-inferred': 'Trust-inferred',
  planned: 'Planned',
};

function isUpcoming(status: LineStatus): boolean {
  const period = status.validityPeriods[0];
  if (!period) return false;
  return !period.isNow && new Date(period.fromDate).getTime() > Date.now();
}

function isActive(status: LineStatus): boolean {
  return status.validityPeriods.some((period) => period.isNow);
}

/** Active first, then upcoming, then anything else (no validity info at
 * all) — matches the three-way split the filter chips already offer.
 * Within a group, earliest `fromDate` first ("what's happening/starting
 * soonest"). Statuses with no validity period sort last within their
 * group via the `Infinity` fallback, rather than erroring or floating to
 * the front. */
function sortRank(status: LineStatus): number {
  if (isActive(status)) return 0;
  if (isUpcoming(status)) return 1;
  return 2;
}

function earliestFromDate(status: LineStatus): number {
  const period = status.validityPeriods[0];
  return period ? new Date(period.fromDate).getTime() : Infinity;
}

function compareByUrgency(a: LineStatus, b: LineStatus): number {
  const rankDiff = sortRank(a) - sortRank(b);
  if (rankDiff !== 0) return rankDiff;
  // `Infinity - Infinity` is `NaN`, an invalid Array.sort comparator
  // result — reached when two statuses in the same group both lack a
  // validity period (only possible in the rank-2 "other" group, which has
  // no date to order by anyway, so treating them as equal is correct).
  return earliestFromDate(a) - earliestFromDate(b) || 0;
}

function formatValiditySummary(status: LineStatus): string {
  const period = status.validityPeriods[0];
  if (!period) return '';
  if (period.isNow) return 'Now';
  const from = new Date(period.fromDate).toLocaleDateString();
  return period.toDate ? `${from} – ${new Date(period.toDate).toLocaleDateString()}` : `From ${from}`;
}

function formatFullValidity(status: LineStatus): string {
  const period = status.validityPeriods[0];
  if (!period) return '';
  const from = new Date(period.fromDate).toLocaleString();
  return period.toDate ? `${from} – ${new Date(period.toDate).toLocaleString()}` : `${from} – ongoing`;
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
 * `tab` is only ever 'active' or 'upcoming' in the final branch: the "All"
 * tab shows the whole chip-filtered pool, so it cannot be empty unless
 * `pool` is 0, which the branch above already caught. */
function emptyStateMessage({
  total,
  pool,
  tab,
  activeCount,
  upcomingCount,
  chipsNarrowing,
}: {
  total: number;
  pool: number;
  tab: ActiveFilter;
  activeCount: number;
  upcomingCount: number;
  chipsNarrowing: boolean;
}): string {
  if (total === 0) return 'No issues reported on this line.';
  if (pool === 0) {
    return `No issues match the selected severity and source filters. Clear a filter to see the other ${pluraliseIssues(total)}.`;
  }

  const lead =
    tab === 'active'
      ? chipsNarrowing
        ? 'No active issues match the selected filters.'
        : 'Nothing is affecting this line right now.'
      : chipsNarrowing
        ? 'No upcoming issues match the selected filters.'
        : 'No issues are scheduled for later on this line.';

  // Name the tab that actually holds something, so the user has somewhere to
  // go; "All" is the catch-all when the sibling tab is empty too.
  const sibling = tab === 'active' ? { label: 'Upcoming', count: upcomingCount } : { label: 'Active', count: activeCount };
  const target = sibling.count > 0 ? sibling : { label: 'All', count: pool };
  return `${lead} ${pluraliseIssues(target.count)} ${target.count === 1 ? 'is' : 'are'} listed under ${target.label}.`;
}

/** Two rows of identical grey pills gave no clue that they were controls,
 * which row meant what, or whether grey was "off" or "everything on". The
 * row label carries both the facet name and its state: an empty selection
 * filters nothing, which is not the same as filtering to nothing. */
function chipRowLabel(facet: string, selected: number): string {
  return selected === 0 ? `${facet} — showing all` : `${facet} — ${selected} selected`;
}

export function IssueList({ statuses }: { statuses: LineStatus[] }) {
  const severityLabelId = useId();
  const sourceLabelId = useId();
  const severityOptions = Array.from(new Set(statuses.map((status) => status.statusSeverityDescription)));
  const [severityFilter, setSeverityFilter] = useState<string[]>([]);
  const [sourceFilter, setSourceFilter] = useState<string[]>([]);
  // Landing tab, decided once on mount (lazy initialiser) rather than
  // derived on every render. "Active" is the right place to open on a line
  // with live disruption, but a line whose issues are all planned/future
  // would open on an empty Active tab while the badge, summary sentence and
  // tab counts all say something is happening — which reads as a bug.
  // "All" is the fallback rather than "Upcoming" because it is the only tab
  // guaranteed to hold everything `statuses` has (an issue can be neither
  // active nor upcoming — a period that started in the past with isNow
  // false — and Upcoming would hide exactly those).
  //
  // Deliberately keyed off `statuses`, not `chipFiltered`, and never
  // recomputed: re-deriving would move the tab under the user the moment a
  // chip toggle happened to empty the current tab. `isActive` reads only the
  // server-supplied `isNow` flag (no `Date.now()`), so server and client
  // initial renders agree.
  const [activeFilter, setActiveFilter] = useState<ActiveFilter>(() =>
    statuses.some(isActive) ? 'active' : 'all',
  );

  // Severity/source chips narrow the pool every tab counts from, but not
  // the active/upcoming tab itself — so switching tabs doesn't change the
  // other tabs' counts, matching a standard faceted-filter count pattern.
  const chipFiltered = statuses.filter((status) => {
    if (severityFilter.length > 0 && !severityFilter.includes(status.statusSeverityDescription)) return false;
    if (sourceFilter.length > 0 && !sourceFilter.includes(status.dataQuality)) return false;
    return true;
  });
  const activeCount = chipFiltered.filter(isActive).length;
  const upcomingCount = chipFiltered.filter(isUpcoming).length;

  const filtered = chipFiltered
    .filter((status) => {
      if (activeFilter === 'active') return isActive(status);
      if (activeFilter === 'upcoming') return isUpcoming(status);
      return true;
    })
    .sort(compareByUrgency);

  return (
    <Stack gap="md">
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
              {Object.entries(DATA_QUALITY_LABELS).map(([value, label]) => (
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
        <SegmentedControl
          value={activeFilter}
          onChange={(value) => setActiveFilter(value as ActiveFilter)}
          data={[
            { label: `All (${chipFiltered.length})`, value: 'all' },
            { label: `Active (${activeCount})`, value: 'active' },
            { label: `Upcoming (${upcomingCount})`, value: 'upcoming' },
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
            chipsNarrowing: chipFiltered.length < statuses.length,
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
        {filtered.map((status, i) => (
          <AccordionItem key={i} value={String(i)}>
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
                    {formatValiditySummary(status)}
                  </Text>
                  {/*
                    Explicit gray: without a `color`, Mantine falls back to
                    theme.primaryColor (grape), making this read as branded
                    or interactive. It's provenance, not brand — gray is
                    already how `informational` severity is treated in
                    lib/severity.ts's GROUP_COLOR.
                  */}
                  <Badge variant="outline" size="sm" color="gray">
                    {DATA_QUALITY_LABELS[status.dataQuality]}
                  </Badge>
                </div>
              </div>
            </AccordionControl>
            <AccordionPanel>
              <Stack gap="xs">
                <Text size="sm" c="dimmed">
                  Valid: {formatFullValidity(status)}
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
