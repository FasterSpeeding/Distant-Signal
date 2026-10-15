import { Alert, Text } from '@mantine/core';
import { LoadMoreControl } from './LoadMoreControl';
import { ProvisionalTimetableNote } from './ProvisionalTimetableNote';
import { ServiceRow, ServiceRowList } from './ServiceRow';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { LoadingText } from '@/components/LoadingText';
import { searchRowDetails, searchRowHeadcode, searchRowSummary, searchRowTime } from '@/lib/searchRow';
import { resolvedDate, trackNoun, type TrainSearchResults as TrainSearchResultsState } from '@/lib/trainSearch';

/** The train search's results: a prompt, loading, an error, an
 * unpublished timetable, no matches, or the rows with their "Load more"
 * footer. */
export function TrainSearchResults({
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
  stationValid: boolean;
  searching: boolean;
  /** A new search is running: the rows shown are for the previous one. */
  pending?: boolean;
  results: TrainSearchResultsState;
  unpublishedDate: string | null;
  loadingMore: boolean;
  onLoadMore: () => void;
  operatorLookup: ReadonlyMap<string, string> | undefined;
  attachTicketId: number | undefined;
}) {
  if (!stationValid) {
    return (
      <Text size="sm" c="dimmed">
        Enter a station above to search for trains that call there.
      </Text>
    );
  }
  if (searching) {
    return <LoadingText>Searching</LoadingText>;
  }
  if (results === null) {
    return (
      <Text size="sm" c="dimmed">
        Press Search to find trains that call at this station.
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
      </Text>
    );
  }
  // The date the rows on screen were ACTUALLY searched with -- computed
  // once here from `results.date`, never from live `dateValue`. See
  // `resolvedDate`'s own doc comment.
  const displayDate = resolvedDate(results.date);
  return (
    <>
      <ProvisionalTimetableNote provisional={results.provisional} />
      <Text size="sm" c="dimmed">
        Times are when each train leaves the station you searched, from the scheduled timetable, and may be up to 30
        minutes out of date. Open a train to see its live status.
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
      {/* One `ServiceRow` per train, its "Track this train" button in the
       * row's actions slot: the destination is the link to the train's
       * page, so the button is never nested in a link. On a phone the
       * actions wrap onto a line of their own under the destination. */}
      <ServiceRowList
        aria-label="Trains"
        data-train-results
        data-list-pending={pending || undefined}
        aria-busy={pending}
      >
        {results.rows.map((row) => (
          <ServiceRow
            key={`${row.uid}-${row.scheduled ?? ''}`}
            train={searchRowSummary(row)}
            date={displayDate}
            timeOverride={searchRowTime(row)}
            dayOffset={row.dayOffset}
            details={
              [searchRowDetails(row, operatorLookup), searchRowHeadcode(row)].filter(Boolean).join(' · ') || undefined
            }
            actions={
              <TrackThisTrainButton
                uid={row.uid}
                date={displayDate}
                {...(attachTicketId === undefined ? {} : { attachTicketId })}
                size="xs"
                noun={trackNoun(row)}
              />
            }
          />
        ))}
      </ServiceRowList>
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
