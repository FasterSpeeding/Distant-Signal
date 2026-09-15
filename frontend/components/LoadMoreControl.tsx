'use client';

import { Button, Group, Stack, Text } from '@mantine/core';

/** The one "Load more" footer every cursor-paginated results list in this
 * app renders below its rows -- `TrainSearchForm`, `IncidentSearchForm` and
 * `StationTimetable` all page `nextCursor`-style keyset responses the same
 * way (fetch `limit + 1` server-side, `nextCursor` an explicit `null` on the
 * last page: see `queries::search_incidents` /
 * `queries::search_schedule_calling_point_departures`), so they share this
 * footer rather than each re-deciding what "no more rows" looks like.
 *
 * Three states, and the point of the component is that none of them is
 * silence:
 *
 * - `hasMore` -- the button, as before.
 * - end of results (`!hasMore`) -- an explicit end-of-list line instead of
 *   the button simply vanishing. A button that disappears is
 *   indistinguishable from a button that broke; the reader is told the list
 *   is complete.
 * - `failed` -- a load-more request that errored. This deliberately does NOT
 *   read as "you've reached the end": callers keep their cursor on failure
 *   (it is still valid) so the button stays for a retry, with the error
 *   named next to it. Before this component existed each caller nulled its
 *   own cursor on a failed page fetch, which silently ended pagination early
 *   and told the reader nothing.
 *
 * `endMessage` is per-caller so the copy can name what actually ran out
 * ("incidents", "scheduled departures"), matching the tone of this app's
 * other nothing-more-to-show states (e.g. `/track/mine`'s empty state).
 * Callers are expected to keep the shared "You've reached the end — …"
 * opening so the states read alike across pages.
 *
 * Both messages are `role="status"`/`role="alert"` live regions: they appear
 * in response to pressing "Load more", and the button they replace or
 * accompany may be the element that still holds focus, so a screen reader
 * would otherwise get no feedback at all from the press. */
export function LoadMoreControl({
  hasMore,
  loading,
  failed = false,
  onLoadMore,
  endMessage,
}: {
  /** Whether the last page fetched came back with a non-null cursor. */
  hasMore: boolean;
  /** A page fetch is in flight -- drives the button's loading state. */
  loading: boolean;
  /** The last load-more attempt failed and can be retried. */
  failed?: boolean;
  onLoadMore: () => void;
  /** End-of-results copy, e.g. "You've reached the end — no more incidents
   * match these filters." */
  endMessage: string;
}) {
  if (!hasMore) {
    // `failed` with no cursor left to retry with can't happen from any
    // caller today (they all keep the cursor on failure), but reporting the
    // failure still beats claiming the list is complete when it isn't.
    return failed ? (
      <Text size="sm" c="red" role="alert">
        Couldn&apos;t load more results.
      </Text>
    ) : (
      <Text size="sm" c="dimmed" role="status">
        {endMessage}
      </Text>
    );
  }
  return (
    <Stack gap={4}>
      {failed && !loading && (
        <Text size="sm" c="red" role="alert">
          Couldn&apos;t load more results. Try again.
        </Text>
      )}
      <Group>
        <Button variant="default" size="xs" onClick={onLoadMore} disabled={loading} loading={loading}>
          Load more
        </Button>
      </Group>
    </Stack>
  );
}
