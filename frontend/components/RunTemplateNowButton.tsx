'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Stack, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { MaterializeTemplateRequest, MaterializeTemplateResponse } from '@/lib/types';

/** Bug: the Service date field here is free text with no client-side
 * format check -- a garbled date used to sail straight through to
 * `POST /JourneyTemplates/{id}/materialize` and come back as a raw backend
 * 400 shown verbatim in the `error` Alert. Same fix, and same rationale for
 * the round-trip-through-`dayjs`-and-compare check (rather than a bare
 * regex), as `AddJourneyLegButton.tsx`'s identical helper: `dayjs` has no
 * `customParseFormat` plugin installed in this app, so it silently rolls
 * an out-of-range date like "2026-02-30" forward to a real one instead of
 * rejecting it, and a bare `DATE_PATTERN` shape check alone would miss
 * that. */
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
function isValidServiceDate(value: string): boolean {
  if (!DATE_PATTERN.test(value)) return false;
  const parsed = dayjs(value);
  return parsed.isValid() && parsed.format('YYYY-MM-DD') === value;
}

/** "Run now" — the manual, on-demand materialization trigger
 * (`POST /JourneyTemplates/{id}/materialize`), §6 item 3. Defaults its
 * date field to today (`dayjs().format('YYYY-MM-DD')`, same convention
 * `AddJourneyLegButton`'s own `handleOpen` already uses) but always sends
 * it explicitly — there is no implicit "today" on the wire
 * (`MaterializeTemplateRequest.serviceDate` is required, matching
 * `crates/api/src/routes/journey_templates.rs::MaterializeTemplateRequest`'s
 * own doc comment). Clicking this MORE THAN ONCE for the same date is
 * explicitly supported, not blocked — see the Phase B plan's Judgment
 * Call 7 — so this component adds no "already ran today" guard of its
 * own. On success, navigates straight to the freshly-minted journey's own
 * detail page (`/journeys/{journeyId}`) — the natural next step is
 * picking a candidate for each newly-unmatched leg there. */
export function RunTemplateNowButton({ templateId }: { templateId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [serviceDate, setServiceDate] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setServiceDate(dayjs().format('YYYY-MM-DD'));
    setError(null);
    needsLoginState.reset();
    open();
  }

  const serviceDateValid = isValidServiceDate(serviceDate);

  async function handleSubmit() {
    if (!serviceDateValid) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const body: MaterializeTemplateRequest = { serviceDate };
      const response = await fetch(`/api/JourneyTemplates/${templateId}/materialize`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
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
      const result: MaterializeTemplateResponse = await response.json();
      setSubmitting(false);
      close();
      router.push(`/journeys/${result.journeyId}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button onClick={handleOpen}>Run now</Button>
      <Modal opened={opened} onClose={close} title="Run this template">
        <Stack>
          <TextInput
            label="Service date"
            placeholder="YYYY-MM-DD"
            value={serviceDate}
            onChange={(event) => setServiceDate(event.currentTarget.value)}
            error={serviceDate.length > 0 && !serviceDateValid ? 'Must be a valid date (YYYY-MM-DD)' : null}
          />
          {error && <Alert color="red">{error}</Alert>}
          {needsLoginState.needsLogin && (
            <LoginLink underline="always">Log in to run this template</LoginLink>
          )}
          <Button onClick={handleSubmit} disabled={!serviceDateValid} loading={submitting}>
            Create journey
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
