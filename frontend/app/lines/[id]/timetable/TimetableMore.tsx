'use client';

import { useEffect, useRef, useState } from 'react';
import { VisuallyHidden } from '@mantine/core';
import { LoadMoreControl } from '@/components/LoadMoreControl';
import { TextLink } from '@/components/TextLink';
import { formatClock, lineTimeMinute, lineTimetableQuery } from '@/lib/lineTrains';
import type { LineCatalogueStation, LineDirection, LineTimetablePage, LineTimetableTrain } from '@/lib/types';
import { LineTrainRow } from '../LineTrainRow';
import classes from '../LineTrains.module.css';

/** The query of the page's first fetch, repeated for every next page. */
export interface TimetableQuery {
  date: string;
  dir: LineDirection | null;
  from: string | null;
  to: string | null;
  at: string | null;
  scope: string;
  limit: number;
}

/** A row's listed time (`HH:MM`) and its arrival at `to`. */
export function rowTimes(train: LineTimetableTrain): { time: string; arrival: string | undefined } {
  const time = lineTimeMinute(train.time);
  const arrival = lineTimeMinute(train.arrival);
  return {
    time: time === null ? '--:--' : formatClock(time),
    arrival: arrival === null ? undefined : formatClock(arrival),
  };
}

/** The rows after the server-rendered first page: "Load more" fetches the
 * next page through the `/api` proxy and appends it (the shared
 * `LoadMoreControl`, as `StationTimetable` does). Without JavaScript the
 * `<noscript>` link opens the next page instead (`after=` in the URL). */
export function TimetableMore({
  id,
  query,
  initialCursor,
  stations,
  nextHref,
}: {
  id: string;
  query: TimetableQuery;
  initialCursor: string | null;
  stations: LineCatalogueStation[];
  /** The no-JavaScript link to the next page. */
  nextHref: string | null;
}) {
  const [rows, setRows] = useState<LineTimetableTrain[]>([]);
  const [cursor, setCursor] = useState<string | null>(initialCursor);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  const [announcement, setAnnouncement] = useState('');
  const active = useRef<AbortController | null>(null);

  useEffect(() => () => active.current?.abort(), []);

  async function loadMore() {
    if (cursor === null || loading) return;
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    setLoading(true);
    try {
      const qs = lineTimetableQuery({ ...query, after: cursor });
      const response = await fetch(`/api/lines/${encodeURIComponent(id)}/timetable?${qs}`, {
        signal: controller.signal,
      });
      if (controller.signal.aborted) return;
      if (!response.ok) {
        setFailed(true);
        return;
      }
      const page: LineTimetablePage = await response.json();
      if (controller.signal.aborted) return;
      setRows((current) => [...current, ...page.trains]);
      setCursor(page.nextCursor);
      setFailed(false);
      setAnnouncement(`${page.trains.length} more train${page.trains.length === 1 ? '' : 's'} loaded.`);
    } catch {
      if (!controller.signal.aborted) setFailed(true);
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }

  return (
    <>
      {rows.length > 0 && (
        <ul className={classes.list} aria-label="More trains">
          {rows.map((train) => {
            const { time, arrival } = rowTimes(train);
            return (
              <LineTrainRow
                key={train.uid}
                train={train}
                date={query.date}
                stations={stations}
                timeOverride={time}
                arrival={arrival}
              />
            );
          })}
        </ul>
      )}
      <VisuallyHidden role="status" aria-live="polite">
        {announcement}
      </VisuallyHidden>
      <LoadMoreControl
        hasMore={cursor !== null}
        loading={loading}
        failed={failed}
        onLoadMore={() => void loadMore()}
        endMessage="You've reached the end — no more trains on this line that day."
      />
      {nextHref && (
        <noscript>
          <TextLink href={nextHref}>Next trains →</TextLink>
        </noscript>
      )}
    </>
  );
}
