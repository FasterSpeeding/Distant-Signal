'use client';

import { useEffect, useState } from 'react';
import { Group, Stack, Text } from '@mantine/core';
import dayjs from 'dayjs';
import { NationalRailCredit } from './NationalRailCredit';
import { ScheduleRow } from './ScheduleRow';
import { TextLink } from './TextLink';
import { CardTitleButton, ClickableCard } from './ClickableCard';
import { LoadingText } from '@/components/LoadingText';
import { stationLabel } from '@/lib/stationLabel';
import {
  CRS_PATTERN,
  fetchCifPickerRows,
  ldbwsDayOffset,
  matchesDestination,
  matchesOperator,
  matchesScheduledDeparture,
  type CifPickerRow,
  type DepartureRow,
  type Picker,
} from '@/lib/departurePicker';

/** The picker's rows for `originCrs`, refetched whenever it changes.
 * `initialOrigin` seeds `loading` so a form mounted with a valid origin
 * shows "Checking for departures" on its first paint. */
export function useDeparturePicker(
  originCrs: string,
  originValid: boolean,
  initialOrigin: string,
): { picker: Picker; loading: boolean } {
  const [picker, setPicker] = useState<Picker>(null);
  // Initialized from `initialOrigin` (not `false`) so a form mounted with
  // an already-valid pre-filled origin shows "Checking for departures"
  // on the very first paint rather than flashing the `picker === null`
  // "couldn't load" sentence for one render before the effect below runs.
  // Per docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
  // Decision 5.
  const [pickerLoading, setPickerLoading] = useState(() => CRS_PATTERN.test(initialOrigin.trim()));

  // Fetch the live departures picker whenever the origin resolves to a
  // syntactically valid CRS -- same same-origin `/api/*` proxy pattern
  // `searchStations`/`searchTocs` already use (client-safe, no `baseUrl()`
  // import). Per docs/superpowers/specs/2026-09-03-trip-search-design.md
  // Decision 4. Falls back to the CIF-derived schedule-departures picker on
  // a 404, per
  // docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
  // Decision 3.
  useEffect(() => {
    if (!originValid) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- clears the picker when its input stops being valid, before any fetch
      setPicker(null);
      setPickerLoading(false);
      return;
    }
    const controller = new AbortController();
    const crs = originCrs.trim().toUpperCase();
    setPickerLoading(true);

    fetch(`/api/stations/${crs}/departures`, { signal: controller.signal })
      .then((res) => {
        if (res.status === 404) {
          // Fallback ONLY on 404 -- an LDBWS network blip or 500 must NOT
          // silently swap in the CIF picker; `!res.ok` still maps to `null`
          // exactly as today, leaving the picker absent rather than
          // switching sources on an error condition. Per
          // docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
          // Decision 3.
          return fetchCifPickerRows(crs, controller.signal).then((rows) => {
            setPickerLoading(false);
            setPicker(rows === 'unavailable' || rows === null ? rows : { source: 'cif', rows });
          });
        }
        if (!res.ok) {
          setPickerLoading(false);
          setPicker(null);
          return;
        }
        return res.json().then((rows: DepartureRow[]) => {
          setPickerLoading(false);
          setPicker({ source: 'ldbws', rows });
        });
      })
      .catch(() => {
        // Aborted (superseded by a newer origin change) or a genuine
        // network blip -- either way, leave prior `picker` state, same
        // posture as `useSuggestions`. Only flip `pickerLoading` off for a
        // genuine failure of *this* request; an aborted one is about to be
        // superseded by a new effect run that has already set it back to
        // `true`, and unconditionally clearing it here would race that.
        if (!controller.signal.aborted) setPickerLoading(false);
      });
    return () => controller.abort();
  }, [originCrs, originValid]);

  return { picker, loading: pickerLoading };
}

/** The picker container's content, in the priority order documented in
 * docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
 * Decision 4 -- exactly one of six mutually-exclusive states, checked
 * top to bottom. Rows are filtered by `matchesDestination`/
 * `matchesOperator` (Decision 1), and additionally by
 * `matchesScheduledDeparture` (both sources -- see that function's own
 * doc comment) before rendering; a source whose *unfiltered* result was
 * already empty (state 5 below) is distinguished from one that had rows
 * but none survived filtering (the two new sentences inside the
 * `'ldbws'`/`'cif'` branches) -- different honest meanings, different
 * copy.
 *
 * Neither row-list branch below is wrapped in a `ScrollArea`, and that is
 * load-bearing. Both used to be (`<ScrollArea mah={220}
 * offsetScrollbars>`), which did not scroll -- it hard-clipped. A Mantine
 * `ScrollArea` root is `position: relative; overflow: hidden`
 * (`@mantine/core/styles/ScrollArea.css`, `.m_d57069b5`) while its
 * viewport is `height: 100%`. With only `mah` on the root, the root's own
 * `height` stays `auto`, so that `100%` resolves to `auto` too (CSS 2.1
 * §10.5: a percentage height against a content-sized containing block
 * computes to `auto`): the viewport grew to its full content height and
 * so never overflowed *itself* -- nothing scrolled -- while the root
 * clamped to 220px and hid the rest behind `overflow: hidden`. Mantine's
 * own scrollbar is sized from `scrollHeight` vs `clientHeight`, equal
 * here, so it never appeared to hint at it either, and the native one is
 * suppressed (`scrollbar-width: none`).
 *
 * That is worse here than in the two list pages with the same defect
 * (`IncidentSearchForm.tsx`, since fixed, and `TrainSearchForm.tsx`):
 * these rows are `role="button"` pickers, not text. A row is ~30px of
 * pitch (a `size="sm"` line -- 14px at Mantine's `--mantine-line-height-
 * sm: 1.45`, so ~20px -- plus the `Stack`'s 10px `xs` gap), so the 220px
 * cap landed after about seven of them, and every row past that was
 * unselectable -- unreachable by pointer and by wheel, and reachable by
 * keyboard only into a dead end (a browser does scroll an `overflow:
 * hidden` box to reveal a focused descendant, which parked the box at an
 * offset the user had no gesture to undo, hiding the *earlier* rows
 * instead). Silently, too: the rows were in the DOM and in the a11y tree,
 * so nothing said a departure had been hidden.
 *
 * `ScrollArea.Autosize` IS the Mantine component that supports a max
 * height (it wraps the root in a `display: flex` / `flex: 1` /
 * `overflow: hidden` chain, which is what makes the root's height
 * definite), but a bounded scroller isn't wanted here anyway: this picker
 * is rendered in the page flow inside the form (see the `mih={72}`
 * `Stack` in `TrackTrainForm`), not in a popover or a dropdown, so there is no
 * containing box it has to fit. Both sources publish about 10 rows in
 * practice -- CIF by a hard cap (`schedule-reference`'s
 * `MAX_DEPARTURES_PER_STATION = 10`, truncated before publication, which
 * `schedule_network_departures`' own migration header records as
 * "next-10, now-forward-filtered"), LDBWS only by the DEFAULT of
 * `poller-ldbws`'s `--num-rows` flag, which an operator can raise (see
 * `crates/poller-ldbws/src/main.rs`, which explicitly contemplates a
 * configured value "much larger than 10"). So ~290px at worst today,
 * barely past the 220px cap it replaces -- and, unlike that cap, an
 * LDBWS board configured longer degrades into more page to scroll
 * rather than into hidden departures. */
export function DeparturePicker({
  originValid,
  loading: pickerLoading,
  picker,
  destinationCrs,
  operator,
  scheduledDeparture,
  onPickLive: pickDeparture,
  onPickTimetable: pickCifDeparture,
}: {
  originValid: boolean;
  loading: boolean;
  picker: Picker;
  destinationCrs: string;
  operator: string;
  scheduledDeparture: string | null;
  onPickLive: (row: DepartureRow) => void;
  onPickTimetable: (row: CifPickerRow) => void;
}) {
  if (!originValid) {
    return (
      <Text size="sm" c="dimmed">
        Enter an origin station above to see upcoming departures.
      </Text>
    );
  }
  if (pickerLoading) {
    return <LoadingText>Checking for departures</LoadingText>;
  }
  if (picker === null) {
    return (
      <Text size="sm" c="dimmed">
        Couldn&apos;t load departures for this station right now. Enter the details below.
      </Text>
    );
  }
  if (picker === 'unavailable') {
    return (
      <Text size="sm" c="dimmed">
        No departure information is available for this station. Enter the details below.
      </Text>
    );
  }
  if (picker.rows.length === 0) {
    return (
      <Text size="sm" c="dimmed">
        No live departures currently on the board for this station right now.
      </Text>
    );
  }
  if (picker.source === 'ldbws') {
    // `ldbwsDayOffset`, not a bare `0`: without it, a near-midnight row
    // that's really tomorrow (see `ldbwsDayOffset`'s own doc comment)
    // would compare as "today HH:MM", read as already-passed relative to
    // a same-day `scheduledDeparture`, and be silently filtered out here
    // before it could ever be reached to click -- the same exposure
    // `matchesScheduledDeparture`'s own doc comment already documents for
    // the CIF side.
    const now = dayjs();
    const filtered = picker.rows.filter(
      (row) =>
        matchesDestination(row.destinationCrs, destinationCrs) &&
        matchesOperator(row.operator, operator) &&
        matchesScheduledDeparture(row.scheduled, scheduledDeparture, ldbwsDayOffset(row.scheduled, now)),
    );
    if (filtered.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No upcoming departures match the destination and/or operator you&apos;ve entered.
        </Text>
      );
    }
    return (
      // Deliberately NOT wrapped in a `ScrollArea` -- see
      // `DeparturePicker`'s own doc comment for the full reasoning. This
      // branch's rows are `role="button"` pickers, so the clipping this
      // used to cause made them literally unselectable.
      //
      // Rows render via the shared `ScheduleRow` component (not
      // hand-rolled here) -- see that component's own comments for
      // its non-clickable cancelled treatment (struck-through time, red
      // "Cancelled" badge, screen-reader text; never dimmed, for AA).
      <>
        <Stack gap="xs" data-departure-picker-rows>
          {filtered.map((row) => (
            <ScheduleRow
              key={row.serviceId}
              row={{
                key: row.serviceId,
                scheduled: row.scheduled,
                destinationCrs: row.destinationCrs,
                destinationName: row.destinationName,
                operator: row.operator,
                isCancelled: row.isCancelled,
                delayMinutes: row.delayMinutes,
                platform: row.platform,
                plannedPlatform: row.plannedPlatform,
                platformChanged: row.platformChanged,
              }}
              onSelect={() => pickDeparture(row)}
            />
          ))}
        </Stack>
        {/* LEG-23: this is Darwin's live departure board, so the National
            Rail credit sits directly under it. */}
        <NationalRailCredit />
      </>
    );
  }
  // picker.source === 'cif' -- Operator never filters this source
  // (Decision 1's CIF/Operator asymmetry): `matchesOperator` is simply
  // never called here.
  const filtered = picker.rows.filter(
    (row) =>
      matchesDestination(row.destinationCrs, destinationCrs) &&
      matchesScheduledDeparture(row.scheduled, scheduledDeparture, row.dayOffset, row.serviceDate),
  );
  return (
    <>
      <Text size="sm" c="dimmed">
        Live departure boards aren&apos;t available for this station. Showing the scheduled timetable instead. This is
        not live running information and may be up to 30 minutes out of date.
      </Text>
      {filtered.length === 0 ? (
        <Text size="sm" c="dimmed">
          No upcoming scheduled departures match the destination you&apos;ve entered.
        </Text>
      ) : (
        // Deliberately NOT wrapped in a `ScrollArea` -- see
        // `DeparturePicker`'s own doc comment, and the LDBWS branch's own
        // copy of this note. Same `role="button"` rows, same defect.
        <Stack gap="xs" data-departure-picker-rows>
          {filtered.map((row) => (
            <ClickableCard key={row.uid} padding="xs">
              <Group justify="space-between" wrap="nowrap">
                <CardTitleButton onClick={() => pickCifDeparture(row)}>
                  <Text component="span" display="block" size="sm">
                    {/* Public time shown; `pickCifDeparture` pins the
                    working time (`cifDepartureDateTime`). */}
                    {row.publicDeparture ?? row.scheduled}
                    {/* `stationLabel` -- same "Name (CODE)", bare-code-fallback
                    convention as everywhere else in this app (item 5,
                    2026-09-22 UX review follow-up): this used to always
                    show the raw code alone. */}
                    {row.destinationCrs ? ` · ${stationLabel(row.destinationCrs, row.destinationName)}` : ''}
                  </Text>
                </CardTitleButton>
                {/* A secondary action, deliberately separate from the row's
                  own click-to-select behaviour above: this navigates to
                  the train's own public status page
                  (`/train/[uid]/[date]`) WITHOUT filling/submitting the
                  tracking form at all, for a visitor who just wants to
                  look, not track. `row.uid` is a real CIF schedule UID
                  here (unlike the LDBWS branch above, whose
                  `DepartureRow` carries no train UID at all -- Darwin's
                  `serviceId` is a different identifier scheme entirely,
                  and the public page is keyed on the CIF/TRUST one --
                  so that branch has nothing honest to link this action
                  to and doesn't render it).

                  `stopPropagation` on BOTH handlers, not just `onClick`:
                  this link sits inside the row's own `role="button"`
                  `onClick`/`onKeyDown`, so without it, either activation
                  path would ALSO select the row for tracking --
                  a plain click bubbles up to the row's `onClick`, and an
                  Enter/Space keydown on the focused link bubbles up to
                  the row's `onKeyDown` before the browser's own
                  synthesized click on the anchor even fires. The link
                  keeps its own native Enter-to-follow behaviour and
                  remains a normal, independently tab-reachable focus
                  stop -- only the *bubbling into the row* is stopped.

                  The linked date is the row's own `serviceDate`, with no `row.dayOffset` added: the public train
                  page is keyed by `(train_uid, service_date)`, the CIF
                  date the service left its origin, and `dayOffset` only
                  moves a row's departure past midnight, never its
                  service date. A service leaving its origin at 23:30 on
                  D and calling here at 00:20 (`dayOffset: 1`) is D's
                  run; D+1 would be the next night's run of the same uid,
                  or a 404. `serviceDate` is a London date captured at
                  fetch time (2026-09-26 review, finding M8; see
                  `CifPickerRow`). */}
                <TextLink href={`/train/${encodeURIComponent(row.uid)}/${row.serviceDate}`}>View live status</TextLink>
              </Group>
            </ClickableCard>
          ))}
        </Stack>
      )}
    </>
  );
}
