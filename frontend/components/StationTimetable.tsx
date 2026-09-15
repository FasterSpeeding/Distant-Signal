'use client';

import { useState } from 'react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Alert,
  Group,
  Stack,
  Text,
} from '@mantine/core';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::calling_point_departure_json`), reused
 * verbatim from `TrainSearchForm.tsx`'s own (unexported, so redeclared
 * here) row/envelope types -- same route, same response, no new fields.
 * `originCrs`/`destinationCrs` are both nullable: either can be
 * unresolved for a real published schedule. This component never sends
 * `date`, so `destinationArrival`/`destinationArrivalDayOffset` are kept
 * for shape-fidelity with the wire response but unused here, same as
 * `TrainSearchForm.tsx`'s own posture for the latter field. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  stationCrs: string;
  originCrs: string | null;
  destinationCrs: string | null;
  destinationArrival: string | null;
  destinationArrivalDayOffset: number;
}

/** The envelope `GET /public/trains/search` returns -- not a bare array,
 * since the backend paginates a whole day's results. `nextCursor` is an
 * explicit `null` on the last page, never omitted. */
interface TrainSearchResponse {
  results: TrainSearchRow[];
  nextCursor: string | null;
}

/** Four mutually-exclusive states, checked top to bottom by
 * `resultsContent` below -- `'unpublished'` and an empty `rows` array are
 * genuinely different facts (spec §0.4/§3.3: a station outside this
 * feed's coverage must not read the same as "nothing's running right
 * now") and must not be collapsed into one copy. */
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;

/** Today's date, computed once per render for every row's live-status
 * link -- there is no date picker on this stripped-down view (spec
 * §3.2). */
function today(): string {
  return dayjs().format('YYYY-MM-DD');
}

export function StationTimetable({ crs }: { crs: string }) {
  const [expanded, setExpanded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<Results>(null);

  async function loadFirstPage() {
    setLoading(true);
    setResults(null);
    try {
      const response = await fetch(`/api/trains/search?station=${crs.toUpperCase()}`);
      if (response.status === 404) {
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setLoading(false);
    }
  }

  function handleChange(value: string | null) {
    const nowExpanded = value !== null;
    setExpanded(nowExpanded);
    if (nowExpanded) {
      void loadFirstPage();
    }
  }

  function resultsContent() {
    if (loading) {
      return (
        <Text size="sm" c="dimmed">
          Loading scheduled departures…
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Couldn&apos;t load">
          Couldn&apos;t load the scheduled departures right now.
        </Alert>
      );
    }
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          Today&apos;s scheduled timetable data isn&apos;t available yet.
        </Text>
      );
    }
    if (results === null) {
      return null;
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No scheduled departures found for the rest of today.
        </Text>
      );
    }
    const displayDate = today();
    return (
      <Stack gap="xs">
        {results.rows.map((row) => (
          <Group key={`${row.uid}-${row.scheduled}`} justify="space-between" wrap="nowrap">
            <Text size="sm">
              {row.scheduled} · {row.originCrs ?? '?'} → {row.stationCrs} → {row.destinationCrs ?? '?'}
            </Text>
            <TextLink href={`/train/${encodeURIComponent(row.uid)}/${displayDate}`}>
              View live status
            </TextLink>
          </Group>
        ))}
      </Stack>
    );
  }

  return (
    <Accordion keepMounted={false} onChange={handleChange}>
      <AccordionItem value="scheduled-departures">
        <AccordionControl>Scheduled departures</AccordionControl>
        <AccordionPanel>
          <Stack gap="xs">{resultsContent()}</Stack>
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}
