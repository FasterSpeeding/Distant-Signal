'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Autocomplete, Button, Group, Modal, SegmentedControl, Stack, Text, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { DateInput } from '@mantine/dates';
import { PICKER_DATE_FORMAT } from '@/lib/dateFormat';
import { nowInLondon } from '@/lib/londonWallClock';
import type { LondonDateTime } from '@/lib/journeyLegChaining';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginControl } from './LoginControl';
import { TimeFilterInput } from './TimeFilterInput';
import {
  newUntrackedLegDraft,
  UntrackedLegFields,
  untrackedDraftRequest,
  type UntrackedLegDraft,
} from './UntrackedLegFields';
import type { NewJourneyLegRequest, AddJourneyLegResponse } from '@/lib/types';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { resolveStationCode, suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';

type LegMode = NewJourneyLegRequest['mode'];

/** Chains a new leg onto an existing journey (spec §3, multi-leg
 * chaining) -- modeled directly on `AddTrainToGroupButton.tsx`'s
 * button → modal → fetch shape (`'use client'`, `useDisclosure`, a bare
 * same-origin `fetch()` against the `/api/...` proxy, `router.refresh()`
 * on success to re-pull `GET /Journeys/{id}` server-side).
 *
 * Two real differences from that component: this form has two submission
 * modes (an open time-window search vs. a directly-known train), and the
 * window mode's origin field is pre-filled -- never locked -- from
 * `priorDestinationCrs` (the previous leg's destination), re-seeded every
 * time the modal opens rather than only once at mount, so a caller who
 * reopens this button after the journey has grown another leg still gets
 * the CURRENT last stop suggested.
 *
 * Deliberately a minimal first cut, not a full replica of
 * `TrainSearchForm.tsx`'s window-entry UI (no live results list to pick
 * a UID from) -- there is no standalone, embeddable time-window-search
 * component in this codebase to reuse; that UI lives inline inside
 * `TrackTrainForm.tsx`. Plain fields submit the window criteria straight
 * to the backend, which does its own matching
 * (`POST /Journeys/{journeyId}/legs`, `mode: 'window'`).
 *
 * The two station fields are the site's standard station search (the same
 * `Autocomplete` + `useSuggestions(searchStations)` +
 * `suggestionAutocompleteProps` trio as `TrackTrainForm.tsx` and the
 * /stations lookup), so a station can be found by its code or its name.
 * They used to be bare code-only text inputs. The leg API takes CRS codes,
 * so each field's text goes through `resolveStationCode`: a picked
 * suggestion (which writes its code) or a typed code is used as is, and a
 * typed full name resolves to its station's code.
 *
 * The service date and earliest departure are pre-filled the same way from
 * `priorArrival` (the previous leg's timetabled arrival at its destination,
 * `journeyPriorArrival`), so the window starts at the moment the traveller
 * gets off the last train. Exactly that time, with no interchange margin
 * added: the journey payload carries no minimum connection time, and an
 * "at or after" bound that is a few minutes early only offers a train or
 * two more, where a guessed margin could hide the one the traveller wants.
 * Like the origin, both are seeded only in `handleOpen`, never from a
 * render, so a re-render (the journey refreshing under an open dialog)
 * never overwrites what the traveller has typed.
 *
 * A third mode, "Tube, bus or walk" (spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2), adds an untracked leg: a mode, optional line and places, and planned
 * times (`UntrackedLegFields`). It is seeded the same way: its date and
 * "leaves" from `priorArrival`, its "from" from `priorPlaceName`. */
export function AddJourneyLegButton({
  journeyId,
  priorDestinationCrs,
  priorArrival = null,
  priorPlaceName = null,
  beforeLegId = null,
  nextLegStart = null,
  label = 'Add a leg',
  onAdded,
}: {
  journeyId: number;
  /** Insert the new leg in front of this leg (`POST .../legs?before=`)
   * instead of after the last one (spec 2026-10-20 §2.3): for a Tube or
   * walk start added to a journey that already has its trains. The dialog
   * then opens on "Tube, bus or walk" and seeds from `nextLegStart`. */
  beforeLegId?: number | null;
  /** Where and when the leg being inserted before starts: the new leg's
   * default "to" place (and, for a train search, destination station) and
   * arrival. */
  nextLegStart?: { place: string | null; crs: string | null; departure: LondonDateTime | null } | null;
  /** The button's words; "Add a leg" by default. */
  label?: string;
  priorDestinationCrs: string | null;
  /** The previous leg's destination in words (`journeyPriorDestinationName`),
   * the default "from" of a new untracked leg. */
  priorPlaceName?: string | null;
  /** The previous leg's arrival in London time, or `null` when no leg is
   * there to chain from or its arrival isn't known: the service date then
   * defaults to London's today and the earliest departure stays empty. */
  priorArrival?: LondonDateTime | null;
  /** `JourneyCreationFlow.tsx`'s (the `/journeys/new` continuous
   * multi-leg creation page) only caller of this prop: that page keeps
   * the journey it's building as local client state (`fetch`ed from
   * `GET /api/Journeys/{id}` after each leg), not as a server-rendered
   * page `router.refresh()` can re-pull the way this button's other
   * caller (`app/journeys/[id]/page.tsx`) does -- so it needs the raw
   * add-leg response back instead. `undefined` (the default) preserves
   * this component's exact pre-existing behaviour: `router.refresh()`
   * fires unconditionally on success, same as before this prop existed. */
  onAdded?: (result: AddJourneyLegResponse) => void;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [mode, setMode] = useState<LegMode>('window');
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [serviceDate, setServiceDate] = useState('');
  const [trainUid, setTrainUid] = useState('');
  // Window-mode time bounds -- `validate_window_leg`
  // (`crates/api/src/data/journeys.rs`) requires at least one of these four
  // to be set, mirrored client-side below (`isValid`) so a submission that
  // would 400 is caught before it round-trips. Same before/after convention
  // `TrackTrainForm.tsx`'s own window-mode fields use -- see that
  // component's `departFrom`/`departTo`/`arriveFrom`/`arriveTo` and
  // `TimeFilterInput`'s own doc comment for why these are plain strings.
  const [departFrom, setDepartFrom] = useState('');
  const [departTo, setDepartTo] = useState('');
  const [arriveFrom, setArriveFrom] = useState('');
  const [arriveTo, setArriveTo] = useState('');
  // `TimeFilterInput` reports a half-entered time (e.g. "09" with no
  // minutes) as `value === ''`, indistinguishable from untouched -- see
  // that component's own doc comment. Tracked the same way
  // `TrackTrainForm.tsx`'s `windowIncompleteTimes` is, so a half-typed
  // bound blocks submission instead of silently vanishing from the request.
  const { suggestions: originSuggestions, loading: originSuggestionsLoading } = useSuggestions(
    originCrs,
    searchStations,
  );
  const { suggestions: destinationSuggestions, loading: destinationSuggestionsLoading } = useSuggestions(
    destinationCrs,
    searchStations,
  );
  // The "not a station" error waits for the field to lose focus: while
  // someone is still typing a name, it isn't a code yet, and that's fine.
  const [originTouched, setOriginTouched] = useState(false);
  const [destinationTouched, setDestinationTouched] = useState(false);
  const [incompleteTimes, setIncompleteTimes] = useState({
    departFrom: false,
    departTo: false,
    arriveFrom: false,
    arriveTo: false,
  });
  const [untrackedDraft, setUntrackedDraft] = useState<UntrackedLegDraft>(() => newUntrackedLegDraft({ date: '' }));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setError(null);
    needsLoginState.reset();
    setMode(beforeLegId === null ? 'window' : 'untracked');
    // Re-seeded on every open (not just once at mount) -- see this
    // component's own doc comment.
    setOriginCrs(beforeLegId === null ? (priorDestinationCrs ?? '') : '');
    setDestinationCrs(beforeLegId === null ? '' : (nextLegStart?.crs ?? ''));
    setOriginTouched(false);
    setDestinationTouched(false);
    // The previous leg's arrival when known (already London time, and on
    // the next day for an arrival after midnight); otherwise London's
    // today, not the browser's (FE-4): a rail service date.
    setServiceDate(priorArrival?.date ?? nowInLondon().format('YYYY-MM-DD'));
    setTrainUid('');
    setDepartFrom(priorArrival?.time ?? '');
    setDepartTo('');
    setArriveFrom('');
    setArriveTo('');
    setIncompleteTimes({ departFrom: false, departTo: false, arriveFrom: false, arriveTo: false });
    setUntrackedDraft(
      beforeLegId === null
        ? newUntrackedLegDraft({
            date: priorArrival?.date ?? nowInLondon().format('YYYY-MM-DD'),
            leaves: priorArrival?.time ?? '',
            originName: priorPlaceName ?? '',
          })
        : // Inserted before a leg: it ends where and when that leg starts.
          newUntrackedLegDraft({
            date: nextLegStart?.departure?.date ?? nowInLondon().format('YYYY-MM-DD'),
            arrives: nextLegStart?.departure?.time ?? '',
            destinationName: nextLegStart?.place ?? '',
          }),
    );
    open();
  }

  const resolvedOrigin = resolveStationCode(originCrs, originSuggestions);
  const resolvedDestination = resolveStationCode(destinationCrs, destinationSuggestions);

  function currentRequest(): NewJourneyLegRequest | null {
    if (mode === 'untracked') return untrackedDraftRequest(untrackedDraft);
    return mode === 'knownTrain'
      ? { mode: 'knownTrain', trainUid, serviceDate }
      : {
          mode: 'window',
          originCrs: resolvedOrigin ?? '',
          destinationCrs: resolvedDestination ?? '',
          serviceDate,
          departWindow: { after: departFrom || null, before: departTo || null },
          arriveWindow: { after: arriveFrom || null, before: arriveTo || null },
        };
  }

  const windowTimesComplete = !Object.values(incompleteTimes).some(Boolean);
  const windowHasABound =
    departFrom.trim() !== '' || departTo.trim() !== '' || arriveFrom.trim() !== '' || arriveTo.trim() !== '';
  // Review §2.2/M14: same "no visible earliest > latest check" gap
  // `TrackTrainForm`'s own window fields had -- plain string comparison is
  // safe here for the same reason it is there (`"HH:MM"` sorts
  // lexicographically identical to chronologically within one day).
  const windowOrderValid =
    (!departFrom || !departTo || departFrom <= departTo) && (!arriveFrom || !arriveTo || arriveFrom <= arriveTo);

  // A `DateInput` only ever emits a real calendar date ("YYYY-MM-DD"), and
  // isn't clearable, so the one way to be invalid is never having had one.
  const serviceDateValid = serviceDate !== '';
  const originCrsValid = resolvedOrigin !== null;
  const destinationCrsValid = resolvedDestination !== null;
  const originError =
    originTouched && originCrs.trim().length > 0 && !originCrsValid && !originSuggestionsLoading
      ? 'Must be a 3-letter station code'
      : null;
  const destinationError =
    destinationTouched && destinationCrs.trim().length > 0 && !destinationCrsValid && !destinationSuggestionsLoading
      ? 'Must be a 3-letter station code'
      : null;

  const isValid =
    mode === 'untracked'
      ? untrackedDraftRequest(untrackedDraft) !== null
      : mode === 'knownTrain'
        ? trainUid.trim() !== '' && serviceDateValid
        : originCrsValid &&
          destinationCrsValid &&
          serviceDateValid &&
          windowTimesComplete &&
          windowHasABound &&
          windowOrderValid;

  async function handleSubmit() {
    const body = currentRequest();
    if (!isValid || body === null) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const query = beforeLegId === null ? '' : `?before=${beforeLegId}`;
      const response = await fetch(`/api/Journeys/${journeyId}/legs${query}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError(await failureFromResponse('add', 'this leg', response));
        }
        setSubmitting(false);
        return;
      }
      // Read only when `onAdded` wants it (see that prop's own doc
      // comment) -- the default path still just confirms success and lets
      // `router.refresh()` re-pull `GET /Journeys/{id}` server-side, same
      // convention as `AddTrainToGroupButton.tsx`.
      const result = (await response.json()) as AddJourneyLegResponse;
      setSubmitting(false);
      close();
      if (onAdded) {
        onAdded(result);
      } else {
        router.refresh();
      }
    } catch {
      setError(describeFailure('add', 'this leg'));
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        {label}
      </Button>
      <Modal
        opened={opened}
        onClose={close}
        title={beforeLegId === null ? 'Add a leg to this journey' : 'Add a leg at the start'}
      >
        <Stack>
          {/* Review §2.1/I21: aligned with `TrackTrainForm`'s own mode
              toggle, which used to say "Search a time window" while this
              one said "Search by time window" -- two near-identical
              phrasings for the same choice. "I know the train" was already
              here and is the better of the two labels the codebase had for
              the OTHER option, so `TrackTrainForm` adopted it too rather
              than the reverse. */}
          {/* Stacked: three options don't fit side by side in the dialog
              without cutting "Search a time window" short. */}
          <SegmentedControl
            orientation="vertical"
            fullWidth
            value={mode}
            onChange={(value) => setMode(value)}
            data={[
              { label: 'Search a time window', value: 'window' },
              { label: 'I know the train', value: 'knownTrain' },
              { label: 'Tube, bus or walk', value: 'untracked' },
            ]}
          />
          {mode === 'untracked' ? (
            <UntrackedLegFields
              draft={untrackedDraft}
              onChange={setUntrackedDraft}
              priorArrivalTime={priorArrival?.time ?? null}
            />
          ) : (
            // Was free text with a "YYYY-MM-DD" placeholder, which showed
            // visitors an ISO date and let a garbled one reach the API. The
            // request still carries "YYYY-MM-DD".
            <DateInput
              label="Service date"
              valueFormat={PICKER_DATE_FORMAT}
              value={serviceDate || null}
              onChange={(value) => setServiceDate(value ?? '')}
            />
          )}
          {mode === 'window' && (
            <>
              <Autocomplete
                label="From station"
                placeholder="e.g. Woking or WOK"
                value={originCrs}
                onChange={setOriginCrs}
                onBlur={() => setOriginTouched(true)}
                {...suggestionAutocompleteProps(originSuggestions, {
                  query: originCrs,
                  loading: originSuggestionsLoading,
                  noMatchMessage: 'No matching stations',
                })}
                error={originError}
              />
              <Autocomplete
                label="To station"
                placeholder="e.g. London Waterloo or WAT"
                value={destinationCrs}
                onChange={setDestinationCrs}
                onBlur={() => setDestinationTouched(true)}
                {...suggestionAutocompleteProps(destinationSuggestions, {
                  query: destinationCrs,
                  loading: destinationSuggestionsLoading,
                  noMatchMessage: 'No matching stations',
                })}
                error={destinationError}
              />
              {/* Review §2.2/I17: same fix as `TrackTrainForm`'s own window
                  fields -- states the at-least-one-of-four rule up front
                  rather than leaving it to a disabled button with no
                  explanation. */}
              <Text size="sm">At least one of the four times below is required.</Text>
              {/* Review §2.2/M14: matches `TrackTrainForm`'s own four
                  descriptions verbatim ("leaving X"/"reaching Y" rather
                  than the bare "departing"/"arriving" this modal used
                  before) -- one phrasing for one rule, not three across
                  the app. */}
              <Group grow align="flex-start">
                <TimeFilterInput
                  clockHint
                  label="Earliest departure (optional)"
                  name="earliest departure"
                  description={
                    `Only trains leaving ${resolvedOrigin ?? 'the origin'} at or after this time.` +
                    (priorArrival ? ` Your last leg arrives at ${priorArrival.time}.` : '')
                  }
                  value={departFrom}
                  onChange={setDepartFrom}
                  onIncompleteChange={(v) => setIncompleteTimes((c) => ({ ...c, departFrom: v }))}
                  error={null}
                />
                <TimeFilterInput
                  label="Latest departure (optional)"
                  name="latest departure"
                  description={`Only trains leaving ${resolvedOrigin ?? 'the origin'} at or before this time.`}
                  value={departTo}
                  onChange={setDepartTo}
                  onIncompleteChange={(v) => setIncompleteTimes((c) => ({ ...c, departTo: v }))}
                  error={null}
                />
              </Group>
              <Group grow align="flex-start">
                <TimeFilterInput
                  label="Earliest arrival (optional)"
                  name="earliest arrival"
                  description={`Only trains reaching ${resolvedDestination ?? 'the destination'} at or after this time.`}
                  value={arriveFrom}
                  onChange={setArriveFrom}
                  onIncompleteChange={(v) => setIncompleteTimes((c) => ({ ...c, arriveFrom: v }))}
                  error={null}
                />
                <TimeFilterInput
                  label="Latest arrival (optional)"
                  name="latest arrival"
                  description={`Only trains reaching ${resolvedDestination ?? 'the destination'} at or before this time.`}
                  value={arriveTo}
                  onChange={setArriveTo}
                  onIncompleteChange={(v) => setIncompleteTimes((c) => ({ ...c, arriveTo: v }))}
                  error={null}
                />
              </Group>
            </>
          )}
          {mode === 'knownTrain' && (
            <TextInput
              label="Train UID"
              placeholder="e.g. A12345"
              value={trainUid}
              onChange={(event) => setTrainUid(event.currentTarget.value)}
            />
          )}
          {error && <Alert color="red">{error}</Alert>}
          {needsLoginState.needsLogin && <LoginControl underline="always">Log in to add a leg</LoginControl>}
          <Button onClick={handleSubmit} disabled={!isValid} loading={submitting}>
            Add leg
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
