'use client';

import { Box, Button, Group, Stack, Text } from '@mantine/core';

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
 * Both messages share ONE always-mounted `role="status"` live region, empty
 * while there is still a next page, rather than each being its own region
 * mounted only when it has something to say. That is deliberate on two
 * counts. A live region inserted into the DOM together with its text is
 * announced inconsistently across screen readers, whereas text inserted into
 * a region that was already there is announced reliably -- and it is the
 * "already there" case that matters, because these messages appear in
 * response to pressing "Load more". The other half is that a one-page result
 * mounts region and text together on first render, which is exactly the case
 * that should NOT be announced: nothing happened, the reader simply arrived
 * at a short list.
 *
 * What the live region cannot do is preserve focus: reaching the end unmounts
 * the button, and if that button held focus it falls back to the document.
 * The announcement is the mitigation, not a fix -- moving focus onto a
 * non-interactive status line would be its own surprise. */
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
  // Exactly one line, or none. A retry in flight suppresses the previous
  // failure rather than leaving a stale error sitting under a spinner --
  // which is also why callers don't need to clear their own failure flag
  // when they start a retry. "Try again" is only offered when there is
  // still a cursor to retry with; `failed` without one can't happen from
  // any caller today, but reporting the failure still beats claiming the
  // list is complete when it isn't.
  const message =
    failed && !loading
      ? `Couldn't load more results.${hasMore ? ' Try again.' : ''}`
      : !hasMore && !failed
        ? endMessage
        : '';

  return (
    <Stack gap={message ? 4 : 0}>
      {hasMore && (
        <Group>
          <Button variant="default" size="xs" onClick={onLoadMore} disabled={loading} loading={loading}>
            Load more
          </Button>
        </Group>
      )}
      <Box role="status" aria-live="polite">
        {message && (
          <Text size="sm" c={failed ? 'red' : 'dimmed'}>
            {message}
          </Text>
        )}
      </Box>
    </Stack>
  );
}
