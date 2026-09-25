'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Modal, SegmentedControl, Stack, Text, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { TimeFilterInput } from './TimeFilterInput';
import type { NewJourneyLegRequest, AddJourneyLegResponse } from '@/lib/types';

type LegMode = NewJourneyLegRequest['mode'];

// Same convention as `TrainSearchForm.tsx`/`TrackTrainForm.tsx`/
// `TicketEntryForm.tsx`'s own `CRS_PATTERN` -- a bare 3-letter code, checked
// client-side so a malformed value gets a friendly inline message here
// rather than reaching the backend and coming back as a raw 400.
const CRS_PATTERN = /^[A-Za-z]{3}$/;

/** Bug: this form's Service date field is free text (`placeholder`
 * "YYYY-MM-DD" is only a hint, not enforced) with no client-side format
 * check at all -- unlike this codebase's date-PICKER fields
 * (`TrackTrainForm.tsx`'s `DateInput`), which can't produce a malformed
 * value by construction. A garbled date used to sail straight through to
 * `POST /Journeys/{id}/legs` and come back as a raw backend 400
 * (`validate_window_leg`/`validate_known_train_leg`,
 * `crates/api/src/data/journeys.rs`) shown verbatim in the `error` Alert.
 *
 * Checks BOTH shape and real calendar validity: `dayjs` (no
 * `customParseFormat` plugin is installed in this app) parses an
 * out-of-range date like "2026-02-30" by rolling it forward to March 2nd
 * rather than rejecting it, so `DATE_PATTERN` alone would pass a value the
 * backend would reject. Re-formatting the parsed result and comparing it
 * back to the original string catches that roll-over without adding a new
 * dependency. */
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
function isValidServiceDate(value: string): boolean {
  if (!DATE_PATTERN.test(value)) return false;
  const parsed = dayjs(value);
  return parsed.isValid() && parsed.format('YYYY-MM-DD') === value;
}

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
 * `TrackTrainForm.tsx`. Plain `TextInput`/`SegmentedControl` fields
 * submit the window criteria straight to the backend, which does its own
 * matching (`POST /Journeys/{journeyId}/legs`, `mode: 'window'`). */
export function AddJourneyLegButton({
  journeyId,
  priorDestinationCrs,
  onAdded,
}: {
  journeyId: number;
  priorDestinationCrs: string | null;
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
  const [incompleteTimes, setIncompleteTimes] = useState({
    departFrom: false,
    departTo: false,
    arriveFrom: false,
    arriveTo: false,
  });
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setError(null);
    needsLoginState.reset();
    setMode('window');
    // Re-seeded on every open (not just once at mount) -- see this
    // component's own doc comment.
    setOriginCrs(priorDestinationCrs ?? '');
    setDestinationCrs('');
    setServiceDate(dayjs().format('YYYY-MM-DD'));
    setTrainUid('');
    setDepartFrom('');
    setDepartTo('');
    setArriveFrom('');
    setArriveTo('');
    setIncompleteTimes({ departFrom: false, departTo: false, arriveFrom: false, arriveTo: false });
    open();
  }

  function currentRequest(): NewJourneyLegRequest {
    return mode === 'knownTrain'
      ? { mode: 'knownTrain', trainUid, serviceDate }
      : {
          mode: 'window',
          originCrs,
          destinationCrs,
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

  const serviceDateValid = isValidServiceDate(serviceDate);
  const originCrsValid = CRS_PATTERN.test(originCrs.trim());
  const destinationCrsValid = CRS_PATTERN.test(destinationCrs.trim());

  const isValid =
    mode === 'knownTrain'
      ? trainUid.trim() !== '' && serviceDateValid
      : originCrsValid &&
        destinationCrsValid &&
        serviceDateValid &&
        windowTimesComplete &&
        windowHasABound &&
        windowOrderValid;

  async function handleSubmit() {
    if (!isValid) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/legs`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(currentRequest()),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      // Read only when `onAdded` wants it (see that prop's own doc
      // comment) -- the default path still just confirms success and lets
      // `router.refresh()` re-pull `GET /Journeys/{id}` server-side, same
      // convention as `AddTrainToGroupButton.tsx`.
      const result: AddJourneyLegResponse = await response.json();
      setSubmitting(false);
      close();
      if (onAdded) {
        onAdded(result);
      } else {
        router.refresh();
      }
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Add a leg
      </Button>
      <Modal opened={opened} onClose={close} title="Add a leg to this journey">
        <Stack>
          {/* Review §2.1/I21: aligned with `TrackTrainForm`'s own mode
              toggle, which used to say "Search a time window" while this
              one said "Search by time window" -- two near-identical
              phrasings for the same choice. "I know the train" was already
              here and is the better of the two labels the codebase had for
              the OTHER option, so `TrackTrainForm` adopted it too rather
              than the reverse. */}
          <SegmentedControl
            value={mode}
            onChange={(value) => setMode(value as LegMode)}
            data={[
              { label: 'Search a time window', value: 'window' },
              { label: 'I know the train', value: 'knownTrain' },
            ]}
          />
          <TextInput
            label="Service date"
            placeholder="YYYY-MM-DD"
            value={serviceDate}
            onChange={(event) => setServiceDate(event.currentTarget.value)}
            error={serviceDate.length > 0 && !serviceDateValid ? 'Must be a valid date (YYYY-MM-DD)' : null}
          />
          {mode === 'window' && (
            <>
              <TextInput
                label="Origin CRS"
                placeholder="e.g. WOK"
                value={originCrs}
                onChange={(event) => setOriginCrs(event.currentTarget.value)}
                error={originCrs.length > 0 && !originCrsValid ? 'Must be a 3-letter CRS code' : null}
              />
              <TextInput
                label="Destination CRS"
                placeholder="e.g. WAT"
                value={destinationCrs}
                onChange={(event) => setDestinationCrs(event.currentTarget.value)}
                error={destinationCrs.length > 0 && !destinationCrsValid ? 'Must be a 3-letter CRS code' : null}
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
                  label="Earliest departure (optional)"
                  name="earliest departure"
                  description={`Only trains leaving ${originCrs.trim() || 'the origin'} at or after this time.`}
                  value={departFrom}
                  onChange={setDepartFrom}
                  onIncompleteChange={(v) => setIncompleteTimes((c) => ({ ...c, departFrom: v }))}
                  error={null}
                />
                <TimeFilterInput
                  label="Latest departure (optional)"
                  name="latest departure"
                  description={`Only trains leaving ${originCrs.trim() || 'the origin'} at or before this time.`}
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
                  description={`Only trains reaching ${destinationCrs.trim() || 'the destination'} at or after this time.`}
                  value={arriveFrom}
                  onChange={setArriveFrom}
                  onIncompleteChange={(v) => setIncompleteTimes((c) => ({ ...c, arriveFrom: v }))}
                  error={null}
                />
                <TimeFilterInput
                  label="Latest arrival (optional)"
                  name="latest arrival"
                  description={`Only trains reaching ${destinationCrs.trim() || 'the destination'} at or before this time.`}
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
          {needsLoginState.needsLogin && <LoginLink underline="always">Log in to add a leg</LoginLink>}
          <Button onClick={handleSubmit} disabled={!isValid} loading={submitting}>
            Add leg
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
