import { Fragment, useId } from 'react';
import { Alert, Text } from '@mantine/core';
import { LoadMoreControl } from './LoadMoreControl';
import { TrackEmptyState } from '@/components/TrackEmptyState';
import { ProvisionalTimetableNote } from './ProvisionalTimetableNote';
import { ServiceRow, ServiceRowList } from './ServiceRow';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { LoadingText } from '@/components/LoadingText';
import type { TrainArrivalResult, TrainSearchResult } from '@/lib/types';
import {
  arrivalRowDetails,
  searchRowDetails,
  searchRowHeadcode,
  searchRowSummary,
  searchRowTime,
} from '@/lib/searchRow';
import {
  nextDateLabel,
  resolvedDate,
  serviceDateLabel,
  splitSearchRows,
  trackNoun,
  type StationBoard,
  type TrainSearchResults as TrainSearchResultsState,
} from '@/lib/trainSearch';

/** Whether a row is an arrivals board row. */
function isArrival(row: TrainSearchResult): row is TrainArrivalResult {
  return typeof (row as Partial<TrainArrivalResult>).arrival === 'string';
}

/** What the results say before there is anything to search. */
function promptCopy(view: 'station' | 'between', board: StationBoard): string {
  if (view === 'between') return 'Pick two stations to see the trains between them.';
  return board === 'arrivals' ? 'Pick a station to see its arrivals.' : 'Pick a station to see its departures.';
}

/** The line above the rows saying which times they show. */
function timesCopy(view: 'station' | 'between', board: StationBoard): string {
  const which =
    view === 'between'
      ? 'Times are when each train leaves the first station, with its arrival at the second'
      : board === 'arrivals'
        ? 'Times are when each train arrives at this station'
        : 'Times are when each train leaves this station';
  return `${which}, from the scheduled timetable, and may be up to 30 minutes out of date. Open a train for its live status, platforms and calling points.`;
}

/** The timetable search's results: a prompt, loading, an error, an
 * unpublished timetable, no matches, or the rows with their "Load more"
 * footer. A station's departures and the trains between two stations are
 * timed at the station searched (between two stations, with the arrival at
 * the second); a station's arrivals by the arrival, headed by where each
 * train comes from. */
export function TrainSearchResults({
  view = 'station',
  board = 'departures',
  stationValid,
  searching,
  pending = false,
  results,
  unpublishedDate,
  loadingMore,
  onLoadMore: handleLoadMore,
  operatorLookup,
  attachTicketId,
}: {
  view?: 'station' | 'between' | undefined;
  board?: StationBoard | undefined;
  /** Whether the station (or both stations) are valid codes. */
  stationValid: boolean;
  searching: boolean;
  /** A new search is running: the rows shown are for the previous one. */
  pending?: boolean;
  results: TrainSearchResultsState;
  unpublishedDate: string | null;
  loadingMore: boolean;
  onLoadMore: () => void;
  operatorLookup: ReadonlyMap<string, string> | undefined;
  /** Only in the ticket flow: each row gets a "Track this train" action
   * that attaches this ticket. Otherwise a row is just its link. */
  attachTicketId: number | undefined;
}) {
  const afterMidnightHeadingId = useId();
  const previousDayHeadingId = useId();
  if (!stationValid) {
    return (
      <Text size="sm" c="dimmed">
        {promptCopy(view, board)}
      </Text>
    );
  }
  if (searching) {
    return <LoadingText>Searching</LoadingText>;
  }
  if (results === null) {
    return (
      <Text size="sm" c="dimmed">
        Press Search to see the timetable.
      </Text>
    );
  }
  if (results === 'error') {
    return (
      <Alert color="red" title="Search failed">
        Couldn&apos;t search for trains right now. Try again.
      </Alert>
    );
  }
  if (results === 'unpublished') {
    return (
      <Text size="sm" c="dimmed">
        {unpublishedDate
          ? `Scheduled timetable data for ${unpublishedDate} isn't available yet. It may not have been published, or that station may not be one this feed covers.`
          : "Today's scheduled timetable data isn't available yet. It may not have been published, or that station may not be one this feed covers."}
      </Text>
    );
  }
  if (results.rows.length === 0) {
    return (
      <Text size="sm" c="dimmed">
        No scheduled trains match those filters right now.
        <TrackEmptyState kind={view === 'between' ? 'trains_between_none' : 'trains_station_board_none'} />
      </Text>
    );
  }
  // The date the rows on screen were ACTUALLY searched with -- computed
  // once here from `results.date`, never from live `dateValue`. See
  // `resolvedDate`'s own doc comment.
  const displayDate = resolvedDate(results.date);
  // An arrival row is split at midnight by its arrival.
  const { previousDay, sameDay, afterMidnight } = splitSearchRows(
    results.rows.map((row) => (isArrival(row) ? { ...row, dayOffset: row.arrivalDayOffset } : row)),
    displayDate,
  );
  // The previous date's trains still running after its midnight (a search
  // with no date picked asks for them; see `apiSearchQuery`), labelled
  // with their own service date, then the searched date's.
  const previousDate = previousDay[0]?.serviceDate;
  const lists: {
    key: 'previous-day' | 'same-day' | 'after-midnight';
    rows: TrainSearchResult[];
    /** The list's heading and its id; `null` for the searched date's own
     * trains, the list named just "Trains". */
    heading: { text: string; id: string; marker: Record<string, boolean> } | null;
  }[] = [
    {
      key: 'previous-day',
      rows: previousDay,
      heading: {
        text: previousDate ? `Still running from ${serviceDateLabel(previousDate)}` : 'Still running',
        id: previousDayHeadingId,
        marker: { 'data-previous-day': true },
      },
    },
    { key: 'same-day', rows: sameDay, heading: null },
    {
      key: 'after-midnight',
      rows: afterMidnight,
      heading: {
        text: `After midnight, ${nextDateLabel(displayDate)}`,
        id: afterMidnightHeadingId,
        marker: { 'data-after-midnight': true },
      },
    },
  ];
  return (
    <>
      <ProvisionalTimetableNote provisional={results.provisional} />
      <Text size="sm" c="dimmed">
        {timesCopy(view, board)}
      </Text>
      {/* Deliberately NOT wrapped in a `ScrollArea` (`mah`-capped or
       * otherwise). It used to be (`<ScrollArea mah={420}
       * offsetScrollbars>`), and that hard-clipped the results: a Mantine
       * `ScrollArea` root is `position: relative; overflow: hidden`
       * (`@mantine/core/styles/ScrollArea.css`, `.m_d57069b5`) while its
       * viewport is `height: 100%`. With only `mah` on the root, the
       * root's own `height` stays `auto`, so that `100%` resolves to
       * `auto` too (CSS 2.1 §10.5: a percentage height against a
       * content-sized containing block computes to `auto`) -- the
       * viewport grows to its full content height and therefore never
       * overflows *itself*, so it never scrolls, while the root clamps to
       * the cap and clips everything past it with `overflow: hidden`. No
       * pointer, wheel or scrollbar gesture could reach a row past 420px,
       * and each "Load more" appended rows straight into the clipped
       * region. Nothing hinted anything had been cut off either: Mantine
       * hides the native scrollbar (`scrollbar-width: none`) and draws its
       * own, sized from `scrollHeight` vs `clientHeight` -- equal here --
       * so it never appeared.
       *
       * `ScrollArea.Autosize` IS the Mantine component that supports a max
       * height (it wraps the root in a `display: flex` / `flex: 1` /
       * `overflow: hidden` chain, which is what makes the root's height
       * definite). It is still not what's used: letting the page scroll is
       * what `StationTimetable.tsx` (the other paginated "Load more" list
       * over these same CIF rows) and `IncidentSearchForm.tsx` (which this
       * component's results section mirrors) both already do. A nested
       * scroller buys nothing here -- there are no sticky controls above
       * the list to keep on screen -- while costing real usability on
       * touch, where it steals the page's own scroll gesture. */}
      {/* One `ServiceRow` per train, the whole row its link to the train's
       * page. Only in the ticket flow does a row carry an action, its
       * "Track this train" button, beside the destination link (never in
       * it); on a phone the actions wrap onto a line of their own. */}
      {/* A date's whole train day: its trains leaving (or, on the arrivals
       * board, arriving) after midnight come last, in a list of their own
       * under the next date, each still marked "+1" and still linking to
       * the searched date's train page (its service date). Before them all,
       * any of the previous date's trains still running (`serviceDate` the
       * day before), under that date, each marked "+1" against it, named as
       * its service, and linking to that date's train page. */}
      {lists.map(({ key, rows, heading }) =>
        rows.length === 0 ? null : (
          <Fragment key={key}>
            {heading && (
              <Text size="sm" fw={600} id={heading.id} {...heading.marker}>
                {heading.text}
              </Text>
            )}
            <ServiceRowList
              {...(heading ? { 'aria-labelledby': heading.id } : { 'aria-label': 'Trains' })}
              data-train-results
              data-list-pending={pending || undefined}
              aria-busy={pending}
            >
              {rows.map((row) => {
                const arrivalRow = isArrival(row) ? row : null;
                const date = row.serviceDate ?? displayDate;
                return (
                  <ServiceRow
                    key={`${row.uid}-${arrivalRow?.arrival ?? row.scheduled ?? ''}`}
                    train={searchRowSummary(row)}
                    headline={arrivalRow ? 'origin' : 'destination'}
                    date={date}
                    timeOverride={arrivalRow ? arrivalRow.arrival : searchRowTime(row)}
                    dayOffset={arrivalRow ? arrivalRow.arrivalDayOffset : (row.dayOffset ?? undefined)}
                    {...(view === 'between' && row.stopsAtArrival
                      ? {
                          arrival: row.stopsAtArrival,
                          arrivalDayOffset: row.stopsAtArrivalDayOffset ?? undefined,
                        }
                      : {})}
                    details={
                      [
                        key === 'previous-day' && row.serviceDate
                          ? `${serviceDateLabel(row.serviceDate)} service`
                          : undefined,
                        arrivalRow
                          ? arrivalRowDetails(arrivalRow, operatorLookup)
                          : searchRowDetails(row, operatorLookup),
                        searchRowHeadcode(row),
                      ]
                        .filter(Boolean)
                        .join(' · ') || undefined
                    }
                    actions={
                      attachTicketId === undefined ? undefined : (
                        <TrackThisTrainButton
                          uid={row.uid}
                          date={date}
                          attachTicketId={attachTicketId}
                          size="xs"
                          noun={trackNoun(row)}
                        />
                      )
                    }
                  />
                );
              })}
            </ServiceRowList>
          </Fragment>
        ),
      )}
      <LoadMoreControl
        hasMore={results.nextCursor !== null}
        loading={loadingMore}
        failed={results.loadMoreFailed}
        onLoadMore={handleLoadMore}
        endMessage="You've reached the end. No more scheduled trains match those filters."
      />
    </>
  );
}
