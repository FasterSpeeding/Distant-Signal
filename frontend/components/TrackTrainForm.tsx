'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, TextInput } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import { TextLink } from './TextLink';
import type { TrackPinRequest, TrackPinResponse } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;

/** The v1 entry point for individual train tracking -- a manual form, not
 * a per-departure "track this train" action, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 1 (no public API exposes individual departures today, so a
 * departure-row action can't be built). `initialOrigin` is set by
 * `/track`'s page when arriving via the "Track a train from here" link on
 * `/stations/[crs]` (Decision 1's honest station-page shortcut).
 *
 * Submits through the same-origin `/api/Train/track` proxy (Client
 * Components can't read the server-only `API_BASE_URL` env var
 * `lib/api.ts` relies on -- same reasoning as `PinToggle`). Mirrors
 * `PinToggle`'s `needsLogin` 401 pattern, with one deliberate difference:
 * a 401 here does NOT reset the form. `PinToggle` can afford to forget its
 * click (there was no typed input to lose); a four-field form has real
 * input worth protecting, so all four fields stay exactly as typed while
 * the login prompt renders alongside them (Decision 4, "no navigation
 * away"). */
export function TrackTrainForm({ initialOrigin = '' }: { initialOrigin?: string }) {
  const router = useRouter();
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState('');
  const [operator, setOperator] = useState('');
  const [scheduledDeparture, setScheduledDeparture] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [needsLogin, setNeedsLogin] = useState(false);
  const [fieldError, setFieldError] = useState<string | null>(null);

  const originValid = CRS_PATTERN.test(originCrs.trim());
  const canSubmit = originValid && scheduledDeparture !== null && !submitting;

  async function handleSubmit() {
    if (!canSubmit || scheduledDeparture === null) return;
    setSubmitting(true);
    setNeedsLogin(false);
    setFieldError(null);
    try {
      const departure = new Date(scheduledDeparture);
      const body: TrackPinRequest = {
        service_date: departure.toISOString().slice(0, 10),
        origin_crs: originCrs.trim().toUpperCase(),
        scheduled_departure: departure.toISOString(),
        ...(destinationCrs.trim() ? { destination_crs: destinationCrs.trim().toUpperCase() } : {}),
        ...(operator.trim() ? { operator: operator.trim() } : {}),
      };

      const response = await fetch('/api/Train/track', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const result: TrackPinResponse = await response.json();
        router.push(`/train/by-id/${result.trackingId}`);
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        setFieldError(await response.text());
        return;
      }
      setFieldError("Couldn't create the tracking pin. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="md">
      <TextInput
        label="Origin CRS code"
        placeholder="e.g. WAT"
        value={originCrs}
        onChange={(event) => setOriginCrs(event.currentTarget.value)}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <DateTimePicker
        label="Scheduled departure"
        placeholder="Pick date and time"
        value={scheduledDeparture}
        onChange={setScheduledDeparture}
        // The backend rejects a departure more than 6 hours in the past
        // (`crates/api/src/data/train_tracking.rs`'s `MAX_PIN_AGE`) --
        // this hint is here so a rejection is rare rather than the
        // user's first encounter with the rule, per Decision 1.
        description="Must be within the last 6 hours, or any time in the future"
        required
      />
      <TextInput
        label="Destination CRS code (optional)"
        placeholder="e.g. WOK"
        value={destinationCrs}
        onChange={(event) => setDestinationCrs(event.currentTarget.value)}
      />
      <TextInput
        label="Operator (optional)"
        placeholder="e.g. SW"
        value={operator}
        onChange={(event) => setOperator(event.currentTarget.value)}
      />
      {fieldError && (
        <Alert color="red" title="Couldn't track this train">
          {fieldError}
        </Alert>
      )}
      <Group>
        <Button onClick={handleSubmit} disabled={!canSubmit}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
        {needsLogin && (
          <TextLink href="/api/auth/login" underline="always">
            Log in to track this train
          </TextLink>
        )}
      </Group>
    </Stack>
  );
}
