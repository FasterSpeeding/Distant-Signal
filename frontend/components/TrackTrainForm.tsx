'use client';

import { useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, TextInput } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import dayjs from 'dayjs';
import { LoginLink } from './LoginLink';
import type { TrackPinRequest, TrackPinResponse } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;

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
 * creating a tracked train for. Once `POST /Train/track` succeeds, this
 * form makes one best-effort follow-up call,
 * `POST /Train/tickets/{attachTicketId}/attach`, before navigating to the
 * new pin's detail page -- if that call fails for any reason (network
 * blip, the ticket having since been attached elsewhere), tracking the
 * train has ALREADY succeeded and this form still navigates on; the ticket
 * just stays standalone and attachable later from the merged trains/tickets
 * list, rather than the whole flow failing over a non-essential follow-up.
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
export function TrackTrainForm({
  initialOrigin = '',
  attachTicketId,
}: {
  initialOrigin?: string;
  attachTicketId?: number;
}) {
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

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit || scheduledDeparture === null) return;
    setSubmitting(true);
    setNeedsLogin(false);
    setFieldError(null);
    try {
      // `scheduledDeparture` is the DateTimePicker's own local-wall-clock
      // string, `'YYYY-MM-DD HH:mm:ss'` (@mantine/dates' `assign-time.mjs`
      // formats it via `date.format('YYYY-MM-DD HH:mm:ss')`) -- not ISO
      // 8601. Its first 10 characters are already the local calendar date
      // the user picked, so `service_date` is read directly off the raw
      // string rather than round-tripped through `Date`/UTC, which would
      // give the wrong day for any departure in the first hour after local
      // midnight while the local UTC offset is positive (e.g. BST). The
      // space-separated form also isn't one of the ECMAScript-guaranteed-
      // parseable date formats (only a `T` separator is), so it's
      // normalized to `'YYYY-MM-DDTHH:mm:ss'` before being handed to `Date`
      // for the (correctly UTC) `scheduled_departure` field.
      const serviceDate = scheduledDeparture.slice(0, 10);
      const departure = new Date(scheduledDeparture.replace(' ', 'T'));
      const body: TrackPinRequest = {
        service_date: serviceDate,
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
        if (attachTicketId !== undefined) {
          // Best-effort: tracking the train has already succeeded above --
          // don't let a failure here (network blip, the ticket having
          // since been attached elsewhere) block navigating to the new
          // pin. The ticket just stays attachable later if this fails.
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed -- see this block's own comment.
          }
        }
        router.push(`/train/by-id/${result.trackingId}`);
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        const text = await response.text();
        setFieldError(text || "Couldn't create the tracking pin. Try again.");
        return;
      }
      setFieldError("Couldn't create the tracking pin. Try again.");
    } catch {
      setFieldError("Couldn't create the tracking pin. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <TextInput
        label="Origin CRS code"
        placeholder="e.g. WAT"
        value={originCrs}
        onChange={(event) => setOriginCrs(event.currentTarget.value)}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <Group align="flex-end" gap="xs">
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
          style={{ flexGrow: 1 }}
        />
        {/* `@mantine/dates`' own `presets` prop (9.5.2) only ever assigns a
            *date* (`DatePickerPreset['value']` is a bare `DateStringValue`,
            like `DatePicker`'s "Today"/"Yesterday" presets) -- it has no
            way to also fill in a time-of-day, so it can't produce "right
            now" on its own; a plain Button next to the picker is the clean
            fit here instead. `dayjs().format('YYYY-MM-DD HH:mm:ss')`
            deliberately matches the exact local-wall-clock string shape
            the picker itself produces (`assign-time.mjs`'s own
            `date.format('YYYY-MM-DD HH:mm:ss')`) -- see this file's own
            `handleSubmit` comment on why that shape, not an ISO string,
            is required to avoid an around-local-midnight day-off-by-one. */}
        <Button variant="default" onClick={() => setScheduledDeparture(dayjs().format('YYYY-MM-DD HH:mm:ss'))}>
          Now
        </Button>
      </Group>
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
        <Button type="submit" disabled={!canSubmit}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
        {needsLogin && (
          <LoginLink underline="always">
            Log in to track this train
          </LoginLink>
        )}
      </Group>
    </Stack>
  );
}
