'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, SegmentedControl, Stack, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { NewJourneyLegRequest, AddJourneyLegResponse } from '@/lib/types';

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
 * `TrackTrainForm.tsx`. Plain `TextInput`/`SegmentedControl` fields
 * submit the window criteria straight to the backend, which does its own
 * matching (`POST /Journeys/{journeyId}/legs`, `mode: 'window'`). */
export function AddJourneyLegButton({
  journeyId,
  priorDestinationCrs,
}: {
  journeyId: number;
  priorDestinationCrs: string | null;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [mode, setMode] = useState<LegMode>('window');
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [serviceDate, setServiceDate] = useState('');
  const [trainUid, setTrainUid] = useState('');
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
    open();
  }

  function currentRequest(): NewJourneyLegRequest {
    return mode === 'knownTrain'
      ? { mode: 'knownTrain', trainUid, serviceDate }
      : { mode: 'window', originCrs, destinationCrs, serviceDate };
  }

  const isValid =
    mode === 'knownTrain'
      ? trainUid.trim() !== '' && serviceDate !== ''
      : originCrs.trim() !== '' && destinationCrs.trim() !== '' && serviceDate !== '';

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
      // Not read further -- `router.refresh()` re-pulls `GET
      // /Journeys/{id}` server-side, same "response is only used to
      // confirm success" convention as `AddTrainToGroupButton.tsx`.
      const _result: AddJourneyLegResponse = await response.json();
      setSubmitting(false);
      close();
      router.refresh();
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
          <SegmentedControl
            value={mode}
            onChange={(value) => setMode(value as LegMode)}
            data={[
              { label: 'Search by time window', value: 'window' },
              { label: 'I know the train', value: 'knownTrain' },
            ]}
          />
          <TextInput
            label="Service date"
            placeholder="YYYY-MM-DD"
            value={serviceDate}
            onChange={(event) => setServiceDate(event.currentTarget.value)}
          />
          {mode === 'window' && (
            <>
              <TextInput
                label="Origin CRS"
                placeholder="e.g. WOK"
                value={originCrs}
                onChange={(event) => setOriginCrs(event.currentTarget.value)}
              />
              <TextInput
                label="Destination CRS"
                placeholder="e.g. WAT"
                value={destinationCrs}
                onChange={(event) => setDestinationCrs(event.currentTarget.value)}
              />
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
