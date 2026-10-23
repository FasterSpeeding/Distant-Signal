'use client';

import { useState } from 'react';
import { Alert, Button, Stack } from '@mantine/core';
import { LoginControl } from './LoginControl';
import { useNeedsLogin } from './useNeedsLogin';
import {
  newUntrackedLegDraft,
  UntrackedLegFields,
  untrackedDraftRequest,
  type UntrackedLegDraft,
} from './UntrackedLegFields';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import { londonToday } from '@/lib/londonWallClock';
import type { CreateJourneyResponse, TransitMode } from '@/lib/types';

/** Where an untracked first leg starts when the page was opened from a TfL
 * stop's "Add as a journey leg" (`/journeys/new?fromTflStop=...`). */
export interface UntrackedLegOrigin {
  stopId: string;
  name: string;
  mode: TransitMode;
}

/** Starts a journey with an untracked leg (a Tube ride, a bus, a walk) on
 * `/journeys/new`: the "Tube, bus or walk" option (spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2.3), for a trip that begins away from the railway. `POST /Journeys`
 * with `mode: "untracked"`, then hands the new journey to `onCreated`, as
 * `TrackTrainForm` and `PlanTripFlow` do for a first train leg. */
export function UntrackedLegStartForm({
  onCreated,
  origin,
}: {
  onCreated: (result: CreateJourneyResponse) => void;
  /** Pre-fills the mode and the From stop. */
  origin?: UntrackedLegOrigin | null | undefined;
}) {
  const [draft, setDraft] = useState<UntrackedLegDraft>(() => {
    const fresh = newUntrackedLegDraft({ date: londonToday() });
    return origin
      ? { ...fresh, transitMode: origin.mode, originName: origin.name, originStopId: origin.stopId }
      : fresh;
  });
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();
  const request = untrackedDraftRequest(draft);

  async function handleSubmit() {
    if (request === null) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ leg: request }),
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
      const result = (await response.json()) as CreateJourneyResponse;
      setSubmitting(false);
      onCreated(result);
    } catch {
      setError(describeFailure('add', 'this leg'));
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="sm">
      <UntrackedLegFields draft={draft} onChange={setDraft} />
      {error && <Alert color="red">{error}</Alert>}
      {needsLoginState.needsLogin && <LoginControl underline="always">Log in to start a journey</LoginControl>}
      <Button onClick={handleSubmit} disabled={request === null} loading={submitting} className="ds-self-start">
        Start journey
      </Button>
    </Stack>
  );
}
