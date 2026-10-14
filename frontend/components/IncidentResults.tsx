import { Alert, Group, Stack, Text } from '@mantine/core';
import { IncidentStateBadge } from './IncidentStateBadge';
import { LoadMoreControl } from './LoadMoreControl';
import { StateBadge } from './StateBadge';
import { TextLink } from './TextLink';
import { LoadingText } from '@/components/LoadingText';
import { formatDateTime } from '@/lib/dateFormat';
import { affectsSummary, type IncidentResults as IncidentResultsState } from '@/lib/incidentSearch';

/** The archive's results column: one of loading, the empty prompt, an
 * error, no matches, or the rows with their "Load more" footer. */
export function IncidentResults({
  results,
  searching,
  loadingMore,
  onLoadMore: handleLoadMore,
  tocNamesByCode,
  lineNamesById,
}: {
  results: IncidentResultsState;
  searching: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  tocNamesByCode: ReadonlyMap<string, string>;
  lineNamesById: ReadonlyMap<string, string>;
}) {
  if (searching) {
    return <LoadingText>Searching</LoadingText>;
  }
  if (results === null) {
    return (
      <Text size="sm" c="dimmed">
        Press Search to browse incidents across the network.
      </Text>
    );
  }
  if (results === 'error') {
    return (
      <Alert color="red" title="Search failed">
        Couldn&apos;t search incidents right now. Try again.
      </Alert>
    );
  }
  if (results.rows.length === 0) {
    return (
      <Text size="sm" c="dimmed">
        No incidents match these filters.
      </Text>
    );
  }
  return (
    <>
      {/* Deliberately NOT wrapped in a `ScrollArea` (`mah`-capped or
       * otherwise). It used to be, and that hard-clipped the archive: a
       * Mantine `ScrollArea` root is `position: relative; overflow: hidden`
       * (`@mantine/core/styles/ScrollArea.css`, `.m_d57069b5`) while its
       * viewport is `height: 100%`. With only `mah` on the root, the root's
       * own `height` stays `auto`, so that `100%` resolves to `auto` too
       * (CSS 2.1 §10.5: a percentage height against a content-sized
       * containing block computes to `auto`) -- the viewport grows to its
       * full content height and therefore never overflows *itself*, so it
       * never scrolls, while the root clamps to the cap and clips
       * everything past it with `overflow: hidden`. Net effect: no pointer,
       * wheel or scrollbar gesture could reach a result past the cap
       * (keyboard focus still could -- browsers scroll an `overflow:
       * hidden` box to reveal a focused descendant -- which left the box
       * parked at an offset with no way back), and each "Load more"
       * appended rows straight into the clipped region. Worse on a ~360px
       * phone, where rows are two or three lines tall so the cap landed
       * after only a handful of them. Nothing hinted that anything had
       * been cut off: Mantine hides the native scrollbar
       * (`scrollbar-width: none`) and draws its own, and that one is
       * sized from `scrollHeight` vs `clientHeight` -- equal here -- so it
       * never appeared either.
       *
       * `ScrollArea.Autosize` IS the Mantine component that supports a max
       * height (it wraps the root in a `display: flex` / `flex: 1` /
       * `overflow: hidden` chain, which is what makes the root's height
       * definite). It is still not used here: letting the page scroll is
       * what `StationTimetable.tsx` -- the other paginated "Load more"
       * list in this app -- already does, rendering its rows as a plain
       * `Stack` with no inner scroll region. (`TrainSearchForm.tsx`, which
       * this component's header says it mirrors, and `TrackTrainForm.tsx`'s
       * departure picker both carried the same `mah`-capped `ScrollArea`
       * and have since been fixed the same way.) A nested scroller buys
       * nothing here anyway --
       * the filter form beside it is short, so there are no sticky controls to
       * preserve -- while costing real usability on touch, where it steals
       * the page's own scroll gesture. */}
      <Stack gap="sm" data-incident-results>
        {results.rows.map((row) => (
          <Stack key={row.incidentId} gap={4}>
            {/* `wrap` is left at Mantine's wrapping default rather than
             * `nowrap`: at ~360px the summary and the timestamp cannot
             * share a line, and forcing them to shrank the timestamp
             * until it broke mid-value ("19 Aug 2026," etc. across four
             * lines) -- under `nowrap` its floor is `min-width: auto`,
             * i.e. its widest *word*, not the whole value. The timestamp
             * instead stays unbreakable and claims the end of whichever
             * line it lands on. `margin-inline-start: auto` rather than
             * the `Group`'s `justify` because `space-between` leaves a
             * *wrapped* single-item line at `flex-start`, which would
             * left-align the date under a long summary on desktop; with
             * the auto margin it reads flush right whether it shares the
             * summary's line or wraps below it.
             *
             * `rowGap` overrides `Group`'s own `md` gap on the wrap axis
             * only (an inline longhand beats the class's `gap` shorthand).
             * Without it a wrapped timestamp sat 16px under its summary
             * while the badge row below sat 4px under the timestamp, so
             * the date read as a label on the badges rather than on the
             * incident it belongs to. The horizontal `md` gap is
             * deliberately left alone.
             *
             * `overflowWrap: 'anywhere'` (inherited, so it reaches the
             * `TextLink` anchor, which takes no style of its own) is what
             * the removed `ScrollArea`'s `overflow: hidden` used to
             * provide by accident: a Knowledgebase summary can contain an
             * unbroken token longer than a 360px screen (a URL, a Welsh
             * station name), and a flex item's `min-width: auto` floor is
             * its longest word -- so without this the row would push the
             * whole page sideways. `anywhere` rather than `break-word`
             * precisely because it DOES lower that intrinsic minimum,
             * which is the whole point here; same choice, same reason, as
             * `.journeyProgressLabel` in `app/globals.css`. */}
            <Group className="incidentRow__head">
              <TextLink href={`/incidents/${encodeURIComponent(row.incidentId)}`} underline="always">
                {row.summary}
              </TextLink>
              <Text size="xs" c="dimmed" ms="auto" className="ds-nowrap">
                {formatDateTime(row.firstSeenAt)}
              </Text>
            </Group>
            {/* At most two badges: planned work (real-time is the default
                and needs no badge) and the incident's state. What it
                affects is plain dimmed text, operators by name. */}
            <Group gap="xs">
              {row.isPlanned && <StateBadge tone="planned">Planned work</StateBadge>}
              <IncidentStateBadge isCleared={row.isCleared} sourceRemovedAt={row.sourceRemovedAt} />
            </Group>
            {affectsSummary(row, tocNamesByCode, lineNamesById) && (
              <Text size="xs" c="dimmed">
                {affectsSummary(row, tocNamesByCode, lineNamesById)}
              </Text>
            )}
          </Stack>
        ))}
      </Stack>
      <LoadMoreControl
        hasMore={results.nextCursor !== null}
        loading={loadingMore}
        failed={results.loadMoreFailed}
        onLoadMore={handleLoadMore}
        endMessage="You've reached the end. No more incidents match these filters."
      />
    </>
  );
}
