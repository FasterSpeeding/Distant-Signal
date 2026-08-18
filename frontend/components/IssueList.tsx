'use client';

import { useState } from 'react';
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

export function IssueList({ statuses }: { statuses: LineStatus[] }) {
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
        <ChipGroup multiple value={severityFilter} onChange={setSeverityFilter}>
          <Group gap="xs">
            {severityOptions.map((option) => (
              <Chip key={option} value={option} size="xs">
                {option}
              </Chip>
            ))}
          </Group>
        </ChipGroup>
        <ChipGroup multiple value={sourceFilter} onChange={setSourceFilter}>
          <Group gap="xs">
            {Object.entries(DATA_QUALITY_LABELS).map(([value, label]) => (
              <Chip key={value} value={value} size="xs">
                {label}
              </Chip>
            ))}
          </Group>
        </ChipGroup>
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

      {filtered.length === 0 && <Text c="dimmed">No issues match the current filters.</Text>}

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
              <Group justify="space-between" wrap="nowrap">
                <Group gap="xs" wrap="nowrap">
                  <StatusBadge severity={status.statusSeverity} />
                  <Text size="sm">{status.reason}</Text>
                </Group>
                <Group gap="xs" wrap="nowrap">
                  <Text size="xs" c="dimmed">
                    {formatValiditySummary(status)}
                  </Text>
                  <Badge variant="outline" size="sm">
                    {DATA_QUALITY_LABELS[status.dataQuality]}
                  </Badge>
                </Group>
              </Group>
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
