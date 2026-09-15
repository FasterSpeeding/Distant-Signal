'use client';

import { useEffect, useRef, useState } from 'react';
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
import { LoadMoreControl } from './LoadMoreControl';
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
  | { rows: TrainSearchRow[]; nextCursor: string | null; loadMoreFailed: boolean }
  | 'unpublished'
  | 'error'
  | null;

/** Today's date, computed once per render for every row's live-status
 * link -- there is no date picker on this stripped-down view (spec
 * §3.2). */
function today(): string {
  return dayjs().format('YYYY-MM-DD');
}

/** Narrows `Results` to the "has rows" branch -- factored out because
 * `loadFirstPage`/`handleLoadMore` need this exact check repeatedly (the
 * early-return guard in `handleLoadMore`, plus each of its three
 * functional `setResults` updaters) and inlining
 * `current !== null && current !== 'error' && current !== 'unpublished'`
 * at every call site invited the checks to drift out of sync. */
function hasRows(
  results: Results,
): results is { rows: TrainSearchRow[]; nextCursor: string | null; loadMoreFailed: boolean } {
  return results !== null && results !== 'error' && results !== 'unpublished';
}

export function StationTimetable({ crs }: { crs: string }) {
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<Results>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  // Guards against the race where a user expands, then collapses and
  // re-expands (or fires "Load more") before the earlier fetch resolves:
  // without this, the stale response's `setResults`/`setLoading` calls
  // could land after the newer request's and overwrite current state.
  // Only one fetch is ever meaningful at a time in this component (first
  // page or next page), so a single ref -- rather than one per call site
  // -- tracking the latest in-flight request is enough: starting any new
  // request aborts whatever was previously in flight and takes over the
  // slot, and `controller.signal.aborted` is re-checked after every
  // `await` so a state-setting call from a superseded request is a no-op
  // instead of corrupting state. Mirrors the `AbortController` +
  // `signal.aborted` convention `lib/useSuggestions.ts` and
  // `TrackTrainForm.tsx`'s departures-picker effect already use for the
  // same kind of stale-response race, adapted here for fetches kicked off
  // from event handlers rather than an effect.
  const activeRequest = useRef<AbortController | null>(null);

  useEffect(() => {
    // Abort any in-flight request if the component unmounts mid-fetch.
    return () => activeRequest.current?.abort();
  }, []);

  /** Aborts whatever is in flight, and clears `loadingMore` with it. The
   * two belong together: an aborted request deliberately skips its own
   * `finally` (see `handleLoadMore`), so if that request was a "Load more"
   * nothing else would ever put the flag back down -- and the next page's
   * button would render permanently disabled and permanently spinning, the
   * exact "button that does nothing" `LoadMoreControl` exists to eliminate.
   * Safe to call unconditionally: `handleLoadMore` raises the flag AFTER
   * `startRequest`. */
  function abortActiveRequest() {
    activeRequest.current?.abort();
    setLoadingMore(false);
  }

  function startRequest(): AbortController {
    abortActiveRequest();
    const controller = new AbortController();
    activeRequest.current = controller;
    return controller;
  }

  async function loadFirstPage() {
    const controller = startRequest();
    setLoading(true);
    setResults(null);
    try {
      const response = await fetch(`/api/trains/search?station=${crs.toUpperCase()}`, {
        signal: controller.signal,
      });
      if (controller.signal.aborted) return;
      if (response.status === 404) {
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      if (controller.signal.aborted) return;
      setResults({ rows: body.results, nextCursor: body.nextCursor, loadMoreFailed: false });
    } catch {
      if (!controller.signal.aborted) setResults('error');
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }

  async function handleLoadMore() {
    if (!hasRows(results) || results.nextCursor === null || loadingMore) return;
    const controller = startRequest();
    const after = results.nextCursor;
    setLoadingMore(true);
    try {
      const response = await fetch(`/api/trains/search?station=${crs.toUpperCase()}&after=${after}`, {
        signal: controller.signal,
      });
      if (controller.signal.aborted) return;
      if (!response.ok) {
        // The cursor is kept, not nulled: this page just failed to load, and
        // the reader gets a named error plus a working retry rather than a
        // list that quietly stops one page short of the end.
        setResults((current) => (hasRows(current) ? { ...current, loadMoreFailed: true } : current));
        return;
      }
      const body: TrainSearchResponse = await response.json();
      if (controller.signal.aborted) return;
      setResults((current) =>
        hasRows(current)
          ? {
              rows: [...current.rows, ...body.results],
              nextCursor: body.nextCursor,
              loadMoreFailed: false,
            }
          : current,
      );
    } catch {
      if (!controller.signal.aborted) {
        setResults((current) => (hasRows(current) ? { ...current, loadMoreFailed: true } : current));
      }
    } finally {
      if (!controller.signal.aborted) setLoadingMore(false);
    }
  }

  function handleChange(value: string | null) {
    if (value !== null) {
      void loadFirstPage();
    } else {
      abortActiveRequest();
      setResults(null);
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
            <TextLink href={`/train/${encodeURIComponent(row.uid)}/${displayDate}`}>View live status</TextLink>
          </Group>
        ))}
        <LoadMoreControl
          hasMore={results.nextCursor !== null}
          loading={loadingMore}
          failed={results.loadMoreFailed}
          onLoadMore={handleLoadMore}
          endMessage="You've reached the end — no more scheduled departures today."
        />
      </Stack>
    );
  }

  return (
    <Stack gap="xs">
      <Accordion keepMounted={false} onChange={handleChange}>
        <AccordionItem value="scheduled-departures">
          <AccordionControl>Scheduled departures</AccordionControl>
          <AccordionPanel>
            <Stack gap="xs">
              <Text size="sm" c="dimmed">
                These are from the scheduled timetable, not live running information, and may be up to 30
                minutes out of date. Open a train to see its live status.
              </Text>
              <Text size="sm" c="dimmed">
                This list shows only departures from this station -- trains that terminate here won&apos;t
                be listed, and neither headcode nor operator is available for scheduled-timetable rows.
              </Text>
              {resultsContent()}
            </Stack>
          </AccordionPanel>
        </AccordionItem>
      </Accordion>
      <TextLink href={`/trains?station=${crs.toUpperCase()}`}>
        Search a different day or filter →
      </TextLink>
    </Stack>
  );
}
