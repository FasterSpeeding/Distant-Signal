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
                <Badge variant="outline" size="sm">
                  {DATA_QUALITY_LABELS[status.dataQuality]}
                </Badge>
              </Group>
            </AccordionControl>
            <AccordionPanel>
              {status.disruption ? (
                <DisruptionDetail disruption={status.disruption} />
              ) : (
                <Text c="dimmed" size="sm">
                  No further detail available.
                </Text>
              )}
            </AccordionPanel>
          </AccordionItem>
        ))}
      </Accordion>
    </Stack>
  );
}
