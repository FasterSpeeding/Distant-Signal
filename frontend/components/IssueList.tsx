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

export function IssueList({ statuses }: { statuses: LineStatus[] }) {
  const severityOptions = Array.from(new Set(statuses.map((status) => status.statusSeverityDescription)));
  const [severityFilter, setSeverityFilter] = useState<string[]>([]);
  const [sourceFilter, setSourceFilter] = useState<string[]>([]);
  const [activeFilter, setActiveFilter] = useState<ActiveFilter>('all');

  const filtered = statuses.filter((status) => {
    if (severityFilter.length > 0 && !severityFilter.includes(status.statusSeverityDescription)) return false;
    if (sourceFilter.length > 0 && !sourceFilter.includes(status.dataQuality)) return false;
    if (activeFilter === 'active' && !isActive(status)) return false;
    if (activeFilter === 'upcoming' && !isUpcoming(status)) return false;
    return true;
  });

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
            { label: 'All', value: 'all' },
            { label: 'Active', value: 'active' },
            { label: 'Upcoming', value: 'upcoming' },
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

        The severity/data-quality badges live in the panel rather than the
        always-visible control header: their text is drawn from the same
        values used for the filter Chip labels above, so surfacing them in
        the header produces two on-screen elements with identical text
        (the chip and the badge) whenever that severity/quality is present —
        ambiguous for anything querying by text, and redundant since the
        chips above already communicate the same information at a glance.
      */}
      {/*
        transitionDuration={0}: paired with keepMounted={false} above, this
        keeps mount/unmount synchronous with the click that triggers it.
        Mantine's Collapse otherwise defers the mount to a
        requestAnimationFrame pair as part of its expand animation, which
        under jsdom lands after the synchronous assertions in this file's
        "expands an entry" test — there's no `await`/`waitFor` there to
        give it room to run.
      */}
      <Accordion multiple keepMounted={false} transitionDuration={0}>
        {filtered.map((status, i) => (
          <AccordionItem key={i} value={String(i)}>
            <AccordionControl>
              <Text size="sm">{status.reason}</Text>
            </AccordionControl>
            <AccordionPanel>
              <Stack gap="xs">
                <Group gap="xs" wrap="nowrap">
                  <StatusBadge severity={status.statusSeverity} />
                  <Badge variant="outline" size="sm">
                    {DATA_QUALITY_LABELS[status.dataQuality]}
                  </Badge>
                </Group>
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
