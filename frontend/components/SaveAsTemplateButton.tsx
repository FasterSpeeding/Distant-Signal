'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Stack, Text, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { CreateJourneyTemplateRequest, CreateJourneyTemplateResponse } from '@/lib/types';

/** "Make this a template" — journey-detail-page header button
 * (docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
 * §6 item 2). Promotes THIS journey's shape into a durable template via
 * `POST /JourneyTemplates` (`mode: 'fromJourney'`) — the backend re-reads
 * this journey's own legs server-side, so this component needs nothing
 * but the journey's id, not its already-fetched leg data (contrast with
 * a hypothetical Phase A "Track this journey again" button, which pre-fills
 * a NEW journey's creation form client-side from data it already has — a
 * different operation entirely, targeting `/journeys/new`, not this
 * route). On success, navigates to the new template's own detail page —
 * the natural next stop, matching `AddJourneyLegButton`'s
 * `router.refresh()`-on-success posture but going further since a whole
 * new resource, not just an update to the current page, was created. */
export function SaveAsTemplateButton({ journeyId }: { journeyId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [customName, setCustomName] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setCustomName('');
    setError(null);
    needsLoginState.reset();
    open();
  }

  async function handleSubmit() {
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const body: CreateJourneyTemplateRequest = {
        mode: 'fromJourney',
        journeyId,
        ...(customName.trim() ? { customName: customName.trim() } : {}),
      };
      const response = await fetch('/api/JourneyTemplates', {
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
      const result: CreateJourneyTemplateResponse = await response.json();
      setSubmitting(false);
      close();
      router.push(`/journeys/templates/${result.templateId}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Make this a template
      </Button>
      <Modal opened={opened} onClose={close} title="Save this journey as a reusable template">
        <Stack>
          <Text size="sm">
            Creates a reusable template from this journey&apos;s route — you can run it again
            for a new date any time from your templates list.
          </Text>
          <TextInput
            label="Template name (optional)"
            placeholder="e.g. Weekday commute"
            value={customName}
            onChange={(event) => setCustomName(event.currentTarget.value)}
          />
          {error && <Alert color="red">{error}</Alert>}
          {needsLoginState.needsLogin && (
            <LoginLink underline="always">Log in to save a template</LoginLink>
          )}
          <Button onClick={handleSubmit} loading={submitting}>
            Save as template
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
