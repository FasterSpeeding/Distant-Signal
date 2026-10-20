'use client';

import { useId, useState, type SubmitEvent } from 'react';
import { useRouter } from 'next/navigation';
import {
  Alert,
  Autocomplete,
  Button,
  Group,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
  VisuallyHidden,
} from '@mantine/core';
import { DateTimePicker, DatePickerInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { DeparturePicker, useDeparturePicker } from './DeparturePicker';
import { TrackDestinationModal } from './TrackDestinationModal';
import { TimeFilterInput } from './TimeFilterInput';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { useGroupSummaries } from '@/lib/useGroupSummaries';
import { shareTrackedTrainToGroup } from '@/lib/shareTrackedTrain';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { nowInLondon, londonWallClockToUtc } from '@/lib/londonWallClock';
import type { CreateJourneyResponse } from '@/lib/types';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import { ButtonSpinner } from '@/components/LoadingText';
import {
  CRS_PATTERN,
  PIN_MAX_DAYS_AHEAD,
  cifDepartureDateTime,
  lastPinDate,
  pinTooFarMessage,
  resolveLdbwsDepartureDate,
  type CifPickerRow,
  type DepartureRow,
} from '@/lib/departurePicker';

/** The v1 entry point for individual train tracking -- a manual form, not
 * a per-departure "track this train" action, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 1 (no public API exposes individual departures today, so a
 * departure-row action can't be built). `initialOrigin` is set by
 * `/track`'s page when arriving via the "Track a train from here" link on
 * `/stations/[crs]` (Decision 1's honest station-page shortcut), OR from
 * `TicketEntryForm`'s own standalone-ticket "next step" link (Part A of the
 * upload-first plan) -- same mechanism, different origin.
 *
 * `attachTicketId`, when given, is a standalone ticket (created via
 * `POST /Train/tickets`, no tracked train yet) the caller is looking for/
 * creating a tracked train for. Once `submitTrack`'s `POST /Journeys`
 * (`pin`-mode leg) call succeeds, this form makes one best-effort follow-up
 * call, `POST /Train/tickets/{attachTicketId}/attach`, before navigating to
 * the new journey's page -- if that call fails for any reason (network
 * blip, the ticket having since been attached elsewhere), tracking the
 * train has ALREADY succeeded and this form still navigates on; the ticket
 * just stays standalone and attachable later from the merged trains/tickets
 * list, rather than the whole flow failing over a non-essential follow-up.
 * `submitWindow`'s open-window leg has no bound train at all yet, so it has
 * no equivalent ticket-attach step.
 *
 * Journey tracking Phase 1 (this task) moved both submit paths off the
 * legacy `POST /Train/track` route onto `POST /Journeys`: `submitTrack`
 * posts a `pin`-mode leg (the pre-existing "pick a departure"/manual-entry
 * flow, field-for-field identical to the old `TrackPinRequest`), and the
 * new `submitWindow` posts a `window`-mode leg for the `SegmentedControl`'s
 * second option -- an open time-window search with no train chosen yet
 * (the journey view, Task 17, is where a candidate gets picked next). Both
 * go through the same same-origin `/api/Journeys` proxy (Client Components
 * can't read the server-only `API_BASE_URL` env var `lib/api.ts` relies on
 * -- same reasoning as `PinToggle`). Mirrors `PinToggle`'s `needsLogin` 401
 * pattern, with one deliberate difference: a 401 here does NOT reset the
 * form. `PinToggle` can afford to forget its click (there was no typed
 * input to lose); a form with real typed input is worth protecting, so
 * every field stays exactly as typed while the login prompt renders
 * alongside it (Decision 4, "no navigation away").
 *
 * Shared-groups follow-up: `useGroupSummaries()` decides, once on mount,
 * whether this user belongs to any group. Zero groups (the majority case,
 * and every anonymous visitor) leaves `handleSubmit`'s form `onSubmit`
 * calling `submitTrack(null)`/`submitWindow(null)` (per `mode`) directly --
 * no prompt, no behavior change from before this feature existed. One or
 * more groups instead makes `handleSubmit` open `TrackDestinationModal`
 * first and defer the actual submit to its `onConfirm`, which calls
 * whichever of the two matches the current `mode` -- see that component's
 * own doc comment for why reusing it unmodified (rather than a second,
 * divergent submit path) is safe. Sharing the new pin into the chosen
 * group (`shareTrackedTrainToGroup`) is a further best-effort follow-up
 * `submitTrack` performs once the track call itself has already
 * succeeded, same swallow-every-failure posture as the `attachTicketId`
 * block right below it -- `submitWindow` has no such follow-up yet (see
 * that function's own doc comment on why `groupId` is currently unused
 * there). */
export function TrackTrainForm({
  initialOrigin = '',
  initialDestination = '',
  attachTicketId,
  initialMode = 'pick',
  initialServiceDate,
  initialDepartAfter = '',
  initialDepartBefore = '',
  initialArriveAfter = '',
  initialArriveBefore = '',
  onCreated,
}: {
  initialOrigin?: string | undefined;
  // "Track this journey again" (docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md)
  // is the first caller of these five props -- pre-fills the Destination
  // field (pin mode and window mode read/write the SAME `destinationCrs`
  // state, see that state declaration's own comment -- only one mode's
  // rendering of it is ever visible at a time, driven by `initialMode`),
  // and the window-mode time bounds. All five are inert, ordinary
  // `useState` initial values, same as `initialOrigin` already is -- no
  // new prop changes this component's submit behaviour.
  initialDestination?: string | undefined;
  attachTicketId?: number | undefined;
  // Review §2.1/I21: the mode toggle used to live only in `useState`, so
  // nothing in the app could send a user straight to window mode -- not
  // even `JourneyLegCard`'s own "Edit search" link. `track/page.tsx` reads
  // this off `?mode=window`, the same pattern its `?origin=` already uses.
  initialMode?: 'pick' | 'window';
  /** "YYYY-MM-DD" -- window mode's own service-date field
   * (`windowServiceDate`, below), which otherwise always defaults to
   * today. `undefined` (the default) preserves that exact pre-existing
   * behaviour. `CreateJourneyLegFromTicketButton.tsx`
   * (`/track?serviceDate=...`) is the first caller of this prop: a
   * ticket's proposed leg is dated by the ticket's own extracted departure
   * date, not necessarily today. Deliberately has no pin-mode equivalent --
   * pin mode's `scheduledDeparture` is a single full date+time field with
   * its own "now" default, not a bare date, so there is nothing for this
   * prop to feed there. */
  initialServiceDate?: string | undefined;
  /** "HH:MM" -- same value contract `TimeFilterInput`'s own `onChange`
   * already uses for `departFrom`/`departTo`/`arriveFrom`/`arriveTo`. */
  initialDepartAfter?: string | undefined;
  initialDepartBefore?: string | undefined;
  initialArriveAfter?: string | undefined;
  initialArriveBefore?: string | undefined;
  /** `JourneyCreationFlow.tsx`'s (the `/journeys/new` continuous
   * multi-leg creation page) only caller of this prop: this form is
   * reused there verbatim as the "leg 1" step, but that page wants to
   * stay put afterwards and offer an inline "Add a leg" rather than being
   * pushed away to `/journeys/{id}` the instant leg 1 exists -- the way
   * every OTHER caller of this form (`/track` itself, chief among them)
   * still wants, and gets, by leaving this prop unset. When given, this
   * replaces `submitTrack`/`submitWindow`'s own final `router.push` with a
   * call to this instead; every other success side effect (the
   * ticket-attach/group-share follow-ups below) is unaffected. `undefined`
   * (the default) preserves this component's exact pre-existing
   * behaviour -- the redirect fires unconditionally, same as before this
   * prop existed. */
  onCreated?: (result: CreateJourneyResponse) => void;
}) {
  const router = useRouter();
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [operator, setOperator] = useState('');
  // Defaults to "now" (the repo owner's own stated expectation), not
  // `null` -- computed once via lazy `useState` initializer, in the exact
  // Europe/London-wall-clock `'YYYY-MM-DD HH:mm:ss'` string shape the "Now"
  // button (below) and `pickDeparture`/`pickCifDeparture` already construct,
  // so it round-trips through `handleSubmit`'s own parsing identically to a
  // value the user picked by hand. `nowInLondon()`, not `dayjs()` -- this
  // value feeds `handleSubmit`'s `serviceDate`/`scheduled_departure`
  // computation directly, so it must be pinned to the London wall clock this
  // whole app treats train times as being in, not whatever zone the
  // visitor's own browser happens to be in (2026-09-26 review, finding M8;
  // contrast the trip planner's "Depart at" field, an ordinary filter
  // input correctly left in plain browser-local time).
  const [scheduledDeparture, setScheduledDeparture] = useState<string | null>(() =>
    nowInLondon().format('YYYY-MM-DD HH:mm:ss'),
  );
  // Darwin's own explicit skipped-calling-point snapshot for whichever
  // live departure-board row the user picked (`pickDeparture`, below) --
  // a live-board pick is the only source that ever has this signal at all
  // (the CIF-picker/manual-entry paths never do). `submitTrack`'s `pin`-mode
  // leg forwards this to `POST /Journeys` (`CreateJourneyLegRequest::Pin`'s
  // own `skippedStations` field, `crates/api/src/routes/journeys.rs`), the
  // same value the legacy `TrackPinRequest` this form used to submit
  // carried under the same name.
  const [skippedStations, setSkippedStations] = useState<string[]>([]);
  // Darwin's own platform snapshot for whichever live departure-board row
  // the user picked (`pickDeparture`, below) -- carried through to the pin
  // so the journey timeline's origin stop can eventually show it (see
  // `common::TrackPinRequest.platform`/`planned_platform`'s own doc
  // comments). `null` (never sent) until an LDBWS row is actually picked --
  // same posture as `skippedStations` immediately above.
  const [platform, setPlatform] = useState<string | null>(null);
  const [plannedPlatform, setPlannedPlatform] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const needsLoginState = useNeedsLogin();
  const [fieldError, setFieldError] = useState<string | null>(null);
  const { groups } = useGroupSummaries();
  const [destinationPromptOpened, setDestinationPromptOpened] = useState(false);

  const { suggestions: originSuggestions, loading: originSuggestionsLoading } = useSuggestions(
    originCrs,
    searchStations,
  );
  const { suggestions: destinationSuggestions, loading: destinationSuggestionsLoading } = useSuggestions(
    destinationCrs,
    searchStations,
  );
  const { suggestions: operatorSuggestions, loading: operatorSuggestionsLoading } = useSuggestions(
    operator,
    searchTocs,
  );
  const [originTouched, setOriginTouched] = useState(false);

  const originValid = CRS_PATTERN.test(originCrs.trim());
  const { picker, loading: pickerLoading } = useDeparturePicker(originCrs, originValid, initialOrigin);
  const canSubmit = originValid && scheduledDeparture !== null && !submitting;

  // Window-search mode -- the `SegmentedControl`'s second option. Reuses
  // `TimeFilterInput`'s existing before/after convention verbatim (per the
  // journey-tracking design doc's own §0.5 direction) rather than inventing
  // a new one for leg-window entry.
  const [mode, setMode] = useState<'pick' | 'window'>(initialMode);
  const modeLabelId = useId();
  // Destination station is genuinely the SAME field in both modes -- one
  // `destinationCrs` state (declared above, alongside `originCrs`), not a
  // second `windowDestinationCrs` copy. It used to be a second copy: two
  // independently-typed `useState`s that both happened to be seeded from
  // `initialDestination`, so a value typed into one mode's Destination
  // field silently vanished the moment the user switched to the other --
  // toggling the `SegmentedControl` looks like "my typing got cleared"
  // even though neither `setState` call is ever cleared by the toggle
  // itself (2026-09-23 state-persistence-across-mode-toggle fix). Unifying
  // the state is what makes the window-mode Autocomplete below able to
  // reuse `destinationSuggestions`/`destinationSuggestionsLoading`
  // (declared with `destinationCrs` above) instead of needing its own
  // dedicated `useSuggestions` instance -- with one shared value there is
  // only one thing for a suggestions dropdown to be driven by, so the
  // reason a second hook instance existed (see this comment's own former
  // text, in git blame) no longer applies.
  //
  // Origin (`originCrs`) already worked this way from the start -- see
  // that field's own "Origin is the one field both modes share" comment
  // just above the `SegmentedControl` in the JSX below -- this brings
  // Destination in line with it. `scheduledDeparture`/window
  // date-and-time-bounds/`operator` stay mode-scoped, deliberately NOT
  // unified: they have no equivalent meaning in the other mode (window
  // mode has no single scheduled-departure instant to fill, pick mode has
  // no depart/arrive-window bounds, and `operator` only narrows pick
  // mode's already-fetched departure-board picker -- window mode has no
  // candidate list of its own to narrow at all, see `submitWindow`'s own
  // doc comment).
  // Review §2.2/M13: seeded with today's real date, not `null` -- a `null`
  // value rendered the field as empty with "Today" as a grey PLACEHOLDER,
  // which reads as "nothing selected" even though `submitWindow` (below)
  // already resolves a `null` to today. Pin-mode's own `scheduledDeparture`
  // field, twenty pixels away in the other branch, shows its default as a
  // real value for the same reason -- this brings window mode in line with
  // it. Still `clearable` (below), and `submitWindow`'s `?? nowInLondon()...`
  // fallback stays as defence if a caller ever clears it back to `null`.
  // `nowInLondon()`, not `dayjs()` -- this is a `serviceDate` sent straight
  // to the backend (same reasoning as `scheduledDeparture`'s own default
  // just above; 2026-09-26 review, finding M8).
  const [windowServiceDate, setWindowServiceDate] = useState<string | null>(
    () => initialServiceDate ?? nowInLondon().format('YYYY-MM-DD'),
  );
  const [departFrom, setDepartFrom] = useState(initialDepartAfter);
  const [departTo, setDepartTo] = useState(initialDepartBefore);
  const [arriveFrom, setArriveFrom] = useState(initialArriveAfter);
  const [arriveTo, setArriveTo] = useState(initialArriveBefore);
  // Same half-entered-time bookkeeping TrainSearchForm.tsx's own four
  // TimeFilterInput fields already need -- see that component's own
  // `incompleteTimes` doc comment for the full reasoning (a native
  // `<input type="time">` reports a half-entered value as `''`,
  // indistinguishable from untouched).
  const [windowIncompleteTimes, setWindowIncompleteTimes] = useState({
    departFrom: false,
    departTo: false,
    arriveFrom: false,
    arriveTo: false,
  });
  // Renamed from `windowDestinationValid` now that Destination is shared
  // state (see the comment above `windowServiceDate`) -- this is no longer
  // a window-mode-only computation, even though it is currently only
  // CONSUMED by window mode's own validation/copy below (pin mode's
  // Destination field stays optional with no equivalent validity gate,
  // unchanged).
  const destinationValid = CRS_PATTERN.test(destinationCrs.trim());
  const windowTimesComplete = !Object.values(windowIncompleteTimes).some(Boolean);
  const windowHasABound =
    departFrom.trim() !== '' || departTo.trim() !== '' || arriveFrom.trim() !== '' || arriveTo.trim() !== '';
  const canSubmitWindow = originValid && destinationValid && windowTimesComplete && windowHasABound && !submitting;

  /** Fills Destination/Operator/Scheduled-departure from a picked, real
   * live departure -- without submitting, so the user can still review/
   * edit before tracking. Combines the departure's `"HH:MM"` with its REAL
   * Europe/London calendar date, via `resolveLdbwsDepartureDate`, into the
   * exact `'YYYY-MM-DD HH:mm:ss'` string shape `scheduledDeparture` already
   * expects -- same construction as the "Now" button above
   * (`nowInLondon().format('YYYY-MM-DD HH:mm:ss')`). `resolveLdbwsDepartureDate`
   * itself converts whatever `now` it's given to Europe/London internally
   * (see that function's own doc comment), so the plain `dayjs()` passed
   * below is fine as an absolute instant -- what would NOT be fine is
   * reading a calendar date or combining it with `row.scheduled` (a London
   * wall-clock reading) anywhere else without going through it.
   *
   * Not always literally "today": a row picked from a live board viewed
   * near local midnight can genuinely be tomorrow (e.g. viewing the board
   * at 23:50 and picking a `"00:07"` row -- a real, near-term, 17-minutes-
   * away departure, not something ~23h43m in the past) --
   * `resolveLdbwsDepartureDate`'s own doc comment explains the heuristic
   * this relies on to tell that case apart from a normal same-day pick. */
  function pickDeparture(row: DepartureRow) {
    setDestinationCrs(row.destinationCrs);
    setOperator(row.operator);
    const [hh, mm] = row.scheduled.split(':');
    const date = resolveLdbwsDepartureDate(row.scheduled, dayjs());
    setScheduledDeparture(`${date} ${hh}:${mm}:00`);
    setSkippedStations(row.skippedStations);
    setPlatform(row.platform);
    setPlannedPlatform(row.plannedPlatform);
  }

  /** CIF-derived sibling of `pickDeparture` -- fills only
   * Destination/Scheduled-departure. `operator` is left exactly as the user
   * already typed it, never cleared, never guessed -- the CIF SCHEDULE feed
   * has no operator field at all (Decision 2). If `row.destinationCrs` is
   * `null` (the terminating TIPLOC has no `stanox_crs` row), the existing
   * Destination field is left untouched too, for the same "never guess,
   * never clobber with a blank" reason.
   *
   * Adds `row.dayOffset` days to the row's own `serviceDate` (a
   * Europe/London date -- 2026-09-26 review, finding M8 -- captured at fetch
   * time, see `CifPickerRow`), rather than always assuming "today" the way
   * `pickDeparture` (LDBWS, below) still does -- a post-midnight CIF calling
   * point (`dayOffset: 1`, e.g. `00:07`) is genuinely TOMORROW relative to
   * the schedule's service date, and combining it with bare "today" would
   * create a pin with the wrong departure instant (see
   * `ScheduleDepartureRow.dayOffset`'s own doc comment).
   *
   * The pin's `serviceDate` (the first ten characters of this value, see
   * `submitTrack`) is therefore the DEPARTURE's London calendar day, not the
   * CIF service date the row's `/train` link uses. That is deliberate and
   * is the backend's documented pin contract: a pin is matched by station +
   * instant, and `ds_store::sweeps::schedule_matching::find_schedule_match`
   * also searches the previous day's overnight schedules for exactly this
   * case, keying the matched train on the schedule's own date. The same
   * contract covers LDBWS and manual pins, which have no service date to
   * send. A row from yesterday's bucket (00:00-02:00, see
   * `fetchCifPickerRows`) departs today and so is pinned with today.
   *
   * `pickDeparture` (LDBWS, above) has the analogous fix,
   * `resolveLdbwsDepartureDate` -- `DepartureRow` still carries no day-offset
   * FIELD (Darwin's live board has no CIF-schedule linkage to derive one
   * from), but unlike here, where the day offset is a known fact read
   * straight off the row, LDBWS instead INFERS it from a bounded-look-ahead
   * heuristic -- see that function's own doc comment for why that's
   * reliable for this specific data source. */
  function pickCifDeparture(row: CifPickerRow) {
    if (row.destinationCrs !== null) setDestinationCrs(row.destinationCrs);
    // The CIF SCHEDULE feed has no per-service skip signal at all (Decision
    // 2) -- clears any snapshot a previously-picked LDBWS row may have left
    // behind, so switching pickers can't carry a stale skip list onto a
    // different service. Same reasoning for platform: CIF has no live
    // platform signal either.
    setSkippedStations([]);
    setPlatform(null);
    setPlannedPlatform(null);
    setScheduledDeparture(cifDepartureDateTime(row));
  }

  /** The real submit -- does the `POST /api/Train/track` call and every
   * follow-up, exactly as this form always has. `groupId` is `null` for the
   * plain personal-tracking flow (the ONLY value it's ever called with when
   * `groups` is empty, preserving today's exact behavior), or a chosen
   * group's id from `TrackDestinationModal`'s `onConfirm`. */
  async function submitTrack(groupId: string | null) {
    if (!canSubmit) return;
    setSubmitting(true);
    needsLoginState.reset();
    setFieldError(null);
    try {
      // `scheduledDeparture` is the DateTimePicker's own bare-wall-clock
      // string, `'YYYY-MM-DD HH:mm:ss'` (@mantine/dates' `assign-time.mjs`
      // formats it via `date.format('YYYY-MM-DD HH:mm:ss')`) -- not ISO
      // 8601, and with no zone/offset of its own. Every value that ever
      // lands in this state (the "now" default, the "Now" button, a picked
      // live-departure-board row, a picked CIF schedule row -- see each of
      // their own doc comments) is built to represent an Europe/London
      // wall-clock reading, matching the departure times themselves (a
      // train's scheduled departure is always stated in Europe/London terms
      // in this app, `lib/dateFormat.ts`'s own stated convention), NOT
      // whatever zone the visitor's own browser happens to be in.
      //
      // `service_date` is `serviceDate`'s first 10 characters, read directly
      // off the raw string rather than round-tripped through `Date` --
      // that's already the London calendar date the string represents, and
      // a round-trip would risk exactly the kind of local-midnight/DST
      // day-off-by-one this comment is about avoiding for the timestamp
      // below.
      //
      // `scheduled_departure`, unlike `service_date`, DOES need the real UTC
      // instant, so it can't just take the string as-is -- but a bare
      // `new Date(scheduledDeparture.replace(' ', 'T'))` (this function's own
      // shape until the 2026-09-26 "Repeater Signal" review, finding M8)
      // parses a zone-less date-time string in the HOST's own timezone per
      // the ECMAScript spec -- the browser's, for a value built client-side.
      // That is only ever correct for a visitor whose own browser happens to
      // be on Europe/London; anyone else (a UK-based visitor travelling
      // abroad, someone tracking a train for a UK-based relative from a
      // different zone) got the wrong instant, silently -- close enough most
      // of the day to go unnoticed, but capable of computing a `/train/
      // {uid}/{date}` link a whole day off `serviceDate` near the London
      // midnight boundary. `londonWallClockToUtc` (`lib/londonWallClock.ts`)
      // is the fix: it explicitly resolves the string against Europe/London
      // (via `dayjs`'s `utc`/`timezone` plugins) rather than relying on
      // whatever zone the runtime happens to be in.
      const serviceDate = scheduledDeparture.slice(0, 10);
      const departure = londonWallClockToUtc(scheduledDeparture);
      const body = {
        customName: null,
        leg: {
          mode: 'pin' as const,
          originCrs: originCrs.trim().toUpperCase(),
          scheduledDeparture: departure.toISOString(),
          serviceDate,
          ...(destinationCrs.trim() ? { destinationCrs: destinationCrs.trim().toUpperCase() } : {}),
          ...(operator.trim() ? { operator: operator.trim() } : {}),
          ...(skippedStations.length > 0 ? { skippedStations } : {}),
          // Integration note (2026-09-22): the Darwin platform snapshot the
          // departure-board picker captures (`pickDeparture`) used to ride
          // on the legacy `POST /Train/track` body as
          // `platform`/`planned_platform`. `POST /Journeys` replaced that
          // route, so the same two values now travel inside the pin-mode
          // LEG, in this endpoint's camelCase convention -- see
          // `CreateJourneyLegRequest::Pin` in
          // `crates/api/src/routes/journeys.rs`, which forwards them into
          // the very same `common::TrackPinRequest` fields the old route
          // filled. Dropping them here would silently lose the origin
          // calling point's platform on every newly pinned journey.
          ...(platform !== null ? { platform } : {}),
          ...(plannedPlatform !== null ? { plannedPlatform } : {}),
        },
      };

      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const result = (await response.json()) as CreateJourneyResponse;
        if (attachTicketId !== undefined && result.trackingId !== null) {
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed -- see this block's own comment above.
          }
        }
        if (groupId !== null && result.trackingId !== null) {
          // Still shares the underlying train_subscriptions row via the
          // EXISTING group_trains table, unchanged -- journey-level
          // sharing is Phase 4 (see this plan's own Non-goals).
          await shareTrackedTrainToGroup(groupId, result.trackingId);
        }
        if (onCreated) {
          onCreated(result);
        } else {
          router.push(`/journeys/${result.journeyId}`);
        }
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        setFieldError(await failureFromResponse('track', 'this train', response));
        return;
      }
      setFieldError(describeFailure('track', 'this train', response.status));
    } catch {
      setFieldError(describeFailure('track', 'this train'));
    } finally {
      setSubmitting(false);
    }
  }

  /** The window-search mode's own submit -- posts a `window`-mode leg to
   * `POST /Journeys` instead of a `pin`-mode one, creating an `'unmatched'`
   * leg with no train bound yet (the journey view, Task 17, is where a
   * candidate gets picked next). Reuses `TrackDestinationModal`'s
   * Personal-vs-group prompt the same way `submitTrack` does -- see
   * `handleSubmit`'s own doc comment -- but `groupId` is currently a
   * placeholder here: sharing an open/unmatched leg into a group at
   * creation time has no real precedent yet (there is no train to share),
   * and this plan's Non-goals explicitly defer journey-level group sharing
   * to Phase 4. The parameter is kept, unused, purely so `handleSubmit`'s
   * dispatch to either `submitTrack`/`submitWindow` can share one call
   * shape without a branch on arity. */
  async function submitWindow(_groupId: string | null) {
    if (!canSubmitWindow) return;
    setSubmitting(true);
    needsLoginState.reset();
    setFieldError(null);
    try {
      const body = {
        customName: null,
        leg: {
          mode: 'window' as const,
          originCrs: originCrs.trim().toUpperCase(),
          destinationCrs: destinationCrs.trim().toUpperCase(),
          serviceDate: windowServiceDate ?? nowInLondon().format('YYYY-MM-DD'),
          departWindow: { after: departFrom || null, before: departTo || null },
          arriveWindow: { after: arriveFrom || null, before: arriveTo || null },
        },
      };
      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (response.ok) {
        const result = (await response.json()) as CreateJourneyResponse;
        // No trackingId yet (an open leg has no train bound) -- so no
        // ticket-attach or group-share follow-up is possible here, unlike
        // submitTrack/pin mode. The journey view itself (Task 17) is where
        // a candidate gets picked next.
        // `_groupId` is reserved for a future group-share-on-window-search follow-up.
        if (onCreated) {
          onCreated(result);
        } else {
          router.push(`/journeys/${result.journeyId}`);
        }
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        setFieldError(await failureFromResponse('search for', 'a train', response));
        return;
      }
      setFieldError(describeFailure('search for', 'a train', response.status));
    } catch {
      setFieldError(describeFailure('search for', 'a train'));
    } finally {
      setSubmitting(false);
    }
  }

  /** The form's own `onSubmit` -- zero groups calls `submitTrack` directly
   * (today's exact behavior, unchanged); one or more groups opens
   * `TrackDestinationModal` instead and defers the real submit to its
   * `onConfirm`. See this component's own doc comment.
   *
   * Task 3.6.14: the submit `Button` below used to stay `disabled` the
   * whole time `!canSubmit` held (i.e. before a valid origin/departure was
   * entered at all), which Mantine renders as light-grey-on-slightly
   * -lighter-grey in dark mode -- under 2:1, and unfixable by a `variant`
   * change alone, since Mantine's own disabled style
   * (`@mantine/core/styles.css`'s `[data-disabled]` rule) unconditionally
   * overrides EVERY variant's background/text/border with that same pair
   * of tokens. Rather than hand-patching `--mantine-color-disabled(-color)`
   * app-wide for one button, this takes the plan's other sanctioned
   * fix: the button now only disables while a submit is actually in
   * flight (`submitting`, a brief, self-explanatory state with its own
   * "Tracking" label) -- an invalid press instead falls through to here
   * and surfaces the same `fieldError` `Alert` a failed backend
   * validation already renders, so a click always gets a visible result
   * instead of silently doing nothing behind an near-invisible control. */
  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    if (submitting) return;
    if (mode === 'window') {
      if (!originValid || !destinationValid || !windowHasABound) {
        setFieldError(
          !originValid || !destinationValid
            ? 'Enter a valid origin and destination station before searching.'
            : 'Enter at least one earliest/latest departure or arrival time to search a window.',
        );
        return;
      }
      // Review §2.2/M14: "no visible earliest > latest check" -- both were
      // previously checked only for PRESENCE, never for order, so
      // "earliest 18:00, latest 09:00" reached the backend unexamined.
      // Plain string comparison is safe here: both are always `"HH:MM"`
      // (`TimeFilterInput`'s own value contract), which sorts
      // lexicographically identical to chronologically within one day.
      if (departFrom && departTo && departFrom > departTo) {
        setFieldError('Latest departure must be after earliest departure.');
        return;
      }
      if (arriveFrom && arriveTo && arriveFrom > arriveTo) {
        setFieldError('Latest arrival must be after earliest arrival.');
        return;
      }
      // The picker's `maxDate` stops a far pick; this catches a prefilled
      // one (`?date=`) before the round trip.
      if (windowServiceDate !== null && windowServiceDate > lastPinDate()) {
        setFieldError(pinTooFarMessage);
        return;
      }
      setFieldError(null);
      if (groups.length > 0) {
        setDestinationPromptOpened(true);
        return;
      }
      void submitWindow(null);
      return;
    }
    if (!originValid || scheduledDeparture === null) {
      setFieldError(
        !originValid
          ? 'Pick an origin station from the suggestions, or enter its 3-letter code.'
          : 'Pick a scheduled departure before tracking.',
      );
      return;
    }
    // The picker's `maxDate` already stops a far pick; this catches a typed
    // or prefilled one before the round trip. `scheduledDeparture` is a
    // London wall-clock string, so its first 10 characters are the
    // service date the backend checks.
    if (scheduledDeparture.slice(0, 10) > lastPinDate()) {
      setFieldError(pinTooFarMessage);
      return;
    }
    setFieldError(null);
    if (groups.length > 0) {
      setDestinationPromptOpened(true);
      return;
    }
    void submitTrack(null);
  }

  return (
    <Stack gap="md">
      {/* Review §2.1/I21: the page's own intro used to be static copy
          owned by `track/page.tsx` ("Pin a specific train…"), true of pick
          mode only -- a visitor who switched to window mode was still told
          the page was for a *specific* train. Lives here, not there, so it
          can react to the client-side mode toggle instead of only the
          initial `?mode=` a page load saw. The ticket-attach case keeps its
          own mode-independent copy from the caller (`track/page.tsx`),
          since attaching a ticket only ever follows the pin-mode path. */}
      {attachTicketId === undefined && (
        <Text c="dimmed">
          {mode === 'window'
            ? "Not sure which train yet? Tell us roughly when you're travelling (Origin, Destination and at least one of the times below) and we'll show you the matches to choose from. You can change your pick later."
            : 'Track a specific train to see its live position, delay and next stop.'}
        </Text>
      )}
      <Stack gap="md" component="form" onSubmit={handleSubmit} maw={640}>
        <Autocomplete
          label="Origin station"
          placeholder="e.g. Woking or WOK"
          value={originCrs}
          onChange={setOriginCrs}
          onBlur={() => setOriginTouched(true)}
          {...suggestionAutocompleteProps(originSuggestions, {
            query: originCrs,
            loading: originSuggestionsLoading,
            noMatchMessage: 'No matching stations',
          })}
          error={originTouched && originCrs.length > 0 && !originValid ? 'Must be a 3-letter station code' : null}
          // NOT the native `required` attribute (Task 3.6.14): an empty
          // origin is now validated by `handleSubmit` itself, which sets
          // `fieldError` and returns before ever calling `submitTrack` --
          // see that function's own doc comment. A native `required` field
          // would make the browser's own constraint validation intercept
          // the submit event before `handleSubmit` ever runs, silently
          // replacing that explanatory message with (at best) a native
          // validation bubble the button's near-invisible disabled state
          // was already standing in for.
        />
        {/* Origin is the one field both modes share -- every leg shape
          (`pin`/`knownTrain`/`window`) needs an `originCrs`, so it stays
          above the mode switch rather than being duplicated inside each
          branch. */}
        {/* Review §2.1/I21: "Pick a departure"/"Search a time window" named
          the FORM's own mechanism, not the traveller's situation -- and the
          sibling `AddJourneyLegButton` modal already used the clearer "I
          know the train" for the equivalent choice, so the codebase had two
          different vocabularies for one decision. Both controls now agree:
          "I know the train" / "Search a time window".

          A visible label wired as the radiogroup's own name. Without it
          a screen reader announced "radiogroup, Pick a departure, radio
          button, 1 of 2, checked" -- the group itself unnamed -- and this
          toggle is the main discovery path for window mode (`?mode=window`
          deep-links into it, but nothing links there), so the name is
          load-bearing (2026-09-22 UX review, I9/P4). Same `Text id` +
          `aria-labelledby` shape
          `HistoryRangePicker`'s "Period" control already uses; the
          question is phrased from the traveller's situation rather than
          from the form's mechanism. */}
        <Text id={modeLabelId} size="xs" fw={600} c="dimmed">
          How do you want to find the train?
        </Text>
        <SegmentedControl
          aria-labelledby={modeLabelId}
          value={mode}
          onChange={(value) => setMode(value)}
          data={[
            { label: 'I know the train', value: 'pick' },
            { label: 'Search a time window', value: 'window' },
          ]}
        />
        {/* Review §2.1/M23: the toggle swaps ~400px of form beneath it with
          no announcement -- sighted users see it happen, screen-reader
          users get nothing until they tab forward into different fields.
          `VisuallyHidden` keeps this out of the visual layout entirely; the
          visible cue (the fields themselves changing) is unaffected. */}
        <VisuallyHidden role="status" aria-live="polite">
          {mode === 'window' ? 'Showing time-window search.' : 'Showing pick-a-departure search.'}
        </VisuallyHidden>
        {mode === 'window' ? (
          <>
            <Autocomplete
              label="Destination station"
              placeholder="e.g. Reading or RDG"
              // `destinationCrs`/`setDestinationCrs`/`destinationSuggestions`
              // -- the SAME state and suggestions hook the pin-mode
              // Destination field below uses, not a separate window-mode
              // copy. See the state-declaration comment above
              // `windowServiceDate` for why: a value typed here now survives
              // switching back to "I know the train" mode, and vice versa.
              value={destinationCrs}
              onChange={setDestinationCrs}
              {...suggestionAutocompleteProps(destinationSuggestions, {
                query: destinationCrs,
                loading: destinationSuggestionsLoading,
                noMatchMessage: 'No matching stations',
              })}
              error={destinationCrs.length > 0 && !destinationValid ? 'Must be a 3-letter station code' : null}
              // NOT the native `required` attribute -- same reasoning as the
              // Origin field's own comment above: a native `required` field
              // would let the browser's own constraint validation intercept
              // the submit event before `handleSubmit` ever runs (confirmed
              // live -- jsdom enforces this too), silently replacing this
              // form's own explanatory `fieldError` message with (at best) a
              // native validation bubble instead. `handleSubmit`'s own
              // `!destinationValid` check already owns this
              // validation.
            />
            <DatePickerInput
              valueFormat="D MMM YYYY"
              label="Date"
              // Review §2.2/M13: `windowServiceDate` is seeded with today's
              // real date above (not `null`), so this now shows an actual
              // value ("22 Sep 2026") the way pin-mode's own default
              // departure does -- "Today" stays only as the placeholder for
              // if the field is ever cleared back to empty.
              placeholder="Today"
              value={windowServiceDate}
              onChange={setWindowServiceDate}
              clearable
              // Bug: this `clearable` field was the one place in the app
              // missing the `clearButtonProps` aria-label every other
              // `clearable` field already carries (see e.g.
              // `TrainSearchForm.tsx`'s own "Clear the date" and
              // `IncidentSearchForm.tsx`'s comment on the same pattern).
              // Mantine's `clearable` renders an icon-only close button with
              // no accessible name of its own, which axe-core flags as a
              // critical `button-name` violation -- caught by the
              // accessibility suite's `/track, departure picker populated`
              // and route-sweep cases once this field had a value to clear.
              clearButtonProps={{ 'aria-label': 'Clear the date' }}
              // The same horizon as a pin: the backend refuses a window
              // leg dated past `PIN_MAX_DAYS_AHEAD` with `pinTooFarMessage`.
              maxDate={lastPinDate()}
            />
            {/* Review §2.2/I17: all four fields below say "(optional)" in
              their own label, which is individually true but collectively
              misleading -- `windowHasABound` (used by `handleSubmit`
              above) refuses to submit unless at least one of the four is
              set, and until now that rule surfaced only as a post-submit
              error. This states it up front instead of relabelling the
              fields (which would have to explain "optional, but not all
              four of you" some other way). */}
            <Text size="sm">At least one of the four times below is required to search.</Text>
            <SimpleGrid cols={{ base: 1, sm: 2 }}>
              <TimeFilterInput
                clockHint
                label="Earliest departure (optional)"
                name="earliest departure"
                description={`Only trains leaving ${originValid ? originCrs.trim().toUpperCase() : 'the origin above'} at or after this time.`}
                value={departFrom}
                onChange={setDepartFrom}
                onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, departFrom: v }))}
                error={null}
              />
              <TimeFilterInput
                label="Latest departure (optional)"
                name="latest departure"
                description={`Only trains leaving ${originValid ? originCrs.trim().toUpperCase() : 'the origin above'} at or before this time.`}
                value={departTo}
                onChange={setDepartTo}
                onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, departTo: v }))}
                error={null}
              />
            </SimpleGrid>
            <SimpleGrid cols={{ base: 1, sm: 2 }}>
              <TimeFilterInput
                label="Earliest arrival (optional)"
                name="earliest arrival"
                description={`Only trains reaching ${destinationValid ? destinationCrs.trim().toUpperCase() : 'the destination above'} at or after this time.`}
                value={arriveFrom}
                onChange={setArriveFrom}
                onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, arriveFrom: v }))}
                error={null}
              />
              <TimeFilterInput
                label="Latest arrival (optional)"
                name="latest arrival"
                description={`Only trains reaching ${destinationValid ? destinationCrs.trim().toUpperCase() : 'the destination above'} at or before this time.`}
                value={arriveTo}
                onChange={setArriveTo}
                onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, arriveTo: v }))}
                error={null}
              />
            </SimpleGrid>
          </>
        ) : (
          <>
            {/* Review §2.3/M12: `wrap="nowrap"` plus the button's own
              `flexShrink: 0` -- without them this `Group` wraps at 390px,
              orphaning "Now" alone on its own row under a full-width
              picker (the same §2.5 shrink-guard idiom `StatusRow` already
              centralises for badge/text rows). */}
            <Group align="flex-end" gap="xs" wrap="nowrap">
              <DateTimePicker
                label="Scheduled departure"
                placeholder="Pick date and time"
                value={scheduledDeparture}
                onChange={setScheduledDeparture}
                // The backend rejects a departure more than 6 hours in the past
                // (`crates/api/src/data/train_tracking.rs`'s `MAX_PIN_AGE`) --
                // this hint is here so a rejection is rare rather than the
                // user's first encounter with the rule, per Decision 1.
                description={`Must be within the last 6 hours, or up to ${PIN_MAX_DAYS_AHEAD} days ahead`}
                // The backend's pin horizon (`PIN_MAX_DAYS_AHEAD`), as the
                // last minute of that London day: Mantine clamps the picked
                // date-time against this on close, so a bare date would
                // turn an evening departure on the last day into midnight.
                maxDate={`${lastPinDate()} 23:59:59`}
                // Same reasoning as the Origin field above -- a cleared
                // departure is validated by `handleSubmit` itself now, not by
                // native `required` constraint validation.
                flex="1 1 auto"
              />
              {/* `@mantine/dates`' own `presets` prop (9.5.2) only ever assigns a
                *date* (`DatePickerPreset['value']` is a bare `DateStringValue`,
                like `DatePicker`'s "Today"/"Yesterday" presets) -- it has no
                way to also fill in a time-of-day, so it can't produce "right
                now" on its own; a plain Button next to the picker is the clean
                fit here instead. `nowInLondon().format('YYYY-MM-DD HH:mm:ss')`
                deliberately matches the exact bare-wall-clock string shape
                the picker itself produces (`assign-time.mjs`'s own
                `date.format('YYYY-MM-DD HH:mm:ss')`) -- see this file's own
                `handleSubmit` comment on why that shape, not an ISO string,
                is required to avoid an around-midnight day-off-by-one, and
                why it's anchored to Europe/London rather than `dayjs()`'s
                host zone (2026-09-26 review, finding M8). */}
              <Button
                variant="default"
                flex="none"
                onClick={() => setScheduledDeparture(nowInLondon().format('YYYY-MM-DD HH:mm:ss'))}
              >
                Now
              </Button>
            </Group>
            <Autocomplete
              label="Destination station (optional)"
              placeholder="e.g. Woking or WOK"
              value={destinationCrs}
              onChange={setDestinationCrs}
              {...suggestionAutocompleteProps(destinationSuggestions, {
                query: destinationCrs,
                loading: destinationSuggestionsLoading,
                noMatchMessage: 'No matching stations',
              })}
            />
            <Autocomplete
              label="Operator (optional)"
              placeholder="e.g. SW"
              value={operator}
              onChange={setOperator}
              {...suggestionAutocompleteProps(operatorSuggestions, {
                query: operator,
                loading: operatorSuggestionsLoading,
                noMatchMessage: 'No matching operators',
              })}
            />
            {/* Always present -- never absent from the DOM, per
              docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
              Decision 4. `mih={72}` blunts the size jump between the
              one/two-line text states; row-list states can legitimately grow
              past the minimum and are deliberately given no maximum -- see
              `DeparturePicker`'s doc comment for why the `mah`-capped
              `ScrollArea` that used to bound them was a clip, not a scroller,
              and why nothing replaced it. */}
            <Stack gap="xs" mih={72}>
              <DeparturePicker
                originValid={originValid}
                loading={pickerLoading}
                picker={picker}
                destinationCrs={destinationCrs}
                operator={operator}
                scheduledDeparture={scheduledDeparture}
                onPickLive={pickDeparture}
                onPickTimetable={pickCifDeparture}
              />
            </Stack>
          </>
        )}
        {fieldError && (
          <Alert color="red" title={mode === 'window' ? "Couldn't search for a train" : "Couldn't track this train"}>
            {fieldError}
          </Alert>
        )}
        <Group>
          {/* Disabled only while a submit is in flight -- see
            `handleSubmit`'s own doc comment (Task 3.6.14) for why an
            invalid-but-not-yet-submitted form no longer disables this
            button at all. */}
          <Button type="submit" disabled={submitting} leftSection={submitting ? <ButtonSpinner /> : undefined}>
            {mode === 'window'
              ? submitting
                ? 'Searching'
                : 'Search for a train'
              : submitting
                ? 'Tracking'
                : 'Track this train'}
          </Button>
        </Group>
      </Stack>
      <TrackDestinationModal
        opened={destinationPromptOpened}
        groups={groups}
        onClose={() => setDestinationPromptOpened(false)}
        onConfirm={(groupId) => void (mode === 'window' ? submitWindow(groupId) : submitTrack(groupId))}
      />
      {/* Review §2.5/M19: the modal used to say "Log in to track this
          train" unconditionally, even in window mode -- where nothing is
          tracked yet (a window search doesn't have a chosen train). */}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        {mode === 'window' ? 'Log in to search for a train.' : 'Log in to track this train.'}
      </LoginPromptModal>
    </Stack>
  );
}
