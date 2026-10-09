'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Flex,
  Stack,
  Text,
  VisuallyHidden,
} from '@mantine/core';
import { nowInLondon } from '@/lib/londonWallClock';
import { searchRowDetails, searchRowSummary, searchRowTime } from '@/lib/searchRow';
import { LoadMoreControl } from './LoadMoreControl';
import { ServiceListNotice, ServiceRow, ServiceRowList } from './ServiceRow';
import { TextLink } from './TextLink';
import type { TrainSearchPage, TrainSearchResult } from '@/lib/types';

/** Four mutually-exclusive states, checked top to bottom by
 * `resultsContent` below -- `'unpublished'` and an empty `rows` array are
 * genuinely different facts (spec §0.4/§3.3: a station outside this
 * feed's coverage must not read the same as "nothing's running right
 * now") and must not be collapsed into one copy. */
type Results =
  { rows: TrainSearchResult[]; nextCursor: string | null; loadMoreFailed: boolean } | 'unpublished' | 'error' | null;

/** Today's London date, computed once per render for every row's
 * live-status link -- there is no date picker on this stripped-down view
 * (spec §3.2). Europe/London, not the browser's zone (FE-4): the link names
 * a rail service date. */
function today(): string {
  return nowInLondon().format('YYYY-MM-DD');
}

/** Narrows `Results` to the "has rows" branch -- factored out because
 * `loadFirstPage`/`handleLoadMore` need this exact check repeatedly (the
 * early-return guard in `handleLoadMore`, plus each of its three
 * functional `setResults` updaters) and inlining
 * `current !== null && current !== 'error' && current !== 'unpublished'`
 * at every call site invited the checks to drift out of sync. */
function hasRows(
  results: Results,
): results is { rows: TrainSearchResult[]; nextCursor: string | null; loadMoreFailed: boolean } {
  return results !== null && results !== 'error' && results !== 'unpublished';
}

/** The station page's "Scheduled departures": the rest of today's
 * departures from `crs` (`GET /public/trains/search`), one `ServiceRow` each,
 * paged with "Load more". `operatorNames` (ATOC code to name, from the
 * page's TOC list) names each row's operator. */
export function StationTimetable({
  crs,
  operatorNames,
}: {
  crs: string;
  operatorNames?: Readonly<Record<string, string>>;
}) {
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<Results>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  // "N more departures loaded", for the polite live region under the list:
  // appended rows are otherwise silent to a screen reader (as
  // `TimetableMore` does on the line timetable).
  const [announcement, setAnnouncement] = useState('');
  const operatorLookup = useMemo(
    () => (operatorNames ? new Map(Object.entries(operatorNames)) : undefined),
    [operatorNames],
  );

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
    setAnnouncement('');
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
      const body = (await response.json()) as TrainSearchPage;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- abort() can land during the await above; TypeScript keeps the pre-await narrowing to false
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
      const body = (await response.json()) as TrainSearchPage;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- abort() can land during the await above; TypeScript keeps the pre-await narrowing to false
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
      setAnnouncement(`${body.results.length} more departure${body.results.length === 1 ? '' : 's'} loaded.`);
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
      return <ServiceListNotice busy>Loading scheduled departures…</ServiceListNotice>;
    }
    if (results === 'error') {
      return <ServiceListNotice>Couldn&apos;t load the scheduled departures right now.</ServiceListNotice>;
    }
    if (results === 'unpublished') {
      return <ServiceListNotice>Today&apos;s scheduled timetable data isn&apos;t available yet.</ServiceListNotice>;
    }
    if (results === null) {
      return null;
    }
    if (results.rows.length === 0) {
      return <ServiceListNotice>No scheduled departures found for the rest of today.</ServiceListNotice>;
    }
    const displayDate = today();
    return (
      <Stack gap="xs">
        <ServiceRowList aria-label="Scheduled departures">
          {results.rows.map((row) => (
            <ServiceRow
              key={`${row.uid}-${row.scheduled ?? ''}`}
              train={searchRowSummary(row)}
              date={displayDate}
              timeOverride={searchRowTime(row)}
              dayOffset={row.dayOffset}
              details={searchRowDetails(row, operatorLookup)}
            />
          ))}
        </ServiceRowList>
        <VisuallyHidden role="status" aria-live="polite">
          {announcement}
        </VisuallyHidden>
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
    // review §3.5.12: this link used to sit as a bare sibling below the
    // accordion, always visible "regardless of expand state" (by design --
    // see this component's own test of that name) but visually floating
    // outside it with nothing connecting the two. Anchored to the control's
    // own row instead -- right-aligned beside it -- rather than moved
    // inside the panel, which would make it disappear whenever the
    // accordion is collapsed and break that same guarantee. Below `sm` it
    // goes under the accordion instead: beside it, a phone left the rows
    // about 230px, too narrow for a destination name.
    <Stack gap="xs">
      <Flex
        direction={{ base: 'column', sm: 'row' }}
        justify="space-between"
        align={{ base: 'stretch', sm: 'flex-start' }}
        gap="sm"
      >
        <Accordion keepMounted={false} onChange={handleChange} style={{ flexGrow: 1, minWidth: 0 }}>
          <AccordionItem value="scheduled-departures">
            <AccordionControl>Scheduled departures</AccordionControl>
            <AccordionPanel>
              <Stack gap="xs">
                <Text size="sm" c="dimmed">
                  These times are from the scheduled timetable and may be up to 30 minutes out of date. Open a train to
                  see its live status.
                </Text>
                <Text size="sm" c="dimmed">
                  Departures only. Trains that end here aren&apos;t listed.
                </Text>
                {resultsContent()}
              </Stack>
            </AccordionPanel>
          </AccordionItem>
        </Accordion>
        {/* In a div: a bare flex item would be stretched full-width in the
            phone column, and so would the link's hit area. */}
        <div>
          <TextLink href={`/trains?station=${crs.toUpperCase()}`} inline underline="always">
            Search a different day or filter <span aria-hidden="true">→</span>
          </TextLink>
        </div>
      </Flex>
    </Stack>
  );
}
