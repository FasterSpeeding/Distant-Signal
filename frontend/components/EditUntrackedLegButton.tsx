'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Stack } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { LoginControl } from './LoginControl';
import { useNeedsLogin } from './useNeedsLogin';
import {
  UntrackedLegFields,
  untrackedDraftFromLeg,
  untrackedDraftRequest,
  type UntrackedLegDraft,
} from './UntrackedLegFields';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import type { JourneyLegDetail } from '@/lib/types';

/** "Edit leg" on an untracked leg's card (spec 2026-10-20 §2.4): the same
 * fields as adding one, seeded from the leg, saved in place with `PUT
 * /Journeys/{id}/legs/{legId}` so the leg keeps its place in the journey.
 * Owner only, like every other leg action. */
export function EditUntrackedLegButton({ journeyId, leg }: { journeyId: number; leg: JourneyLegDetail }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [draft, setDraft] = useState<UntrackedLegDraft>(() => untrackedDraftFromLeg(leg));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();
  const request = untrackedDraftRequest(draft);

  function handleOpen() {
    setDraft(untrackedDraftFromLeg(leg));
    setError(null);
    needsLoginState.reset();
    open();
  }

  async function handleSave() {
    if (request === null) return;
    // The PUT body is the leg's fields without the add request's `mode`.
    const { mode: _mode, ...body } = request;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/legs/${leg.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      setSubmitting(false);
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError(await failureFromResponse('save', 'this leg', response));
        }
        return;
      }
      close();
      router.refresh();
    } catch {
      setError(describeFailure('save', 'this leg'));
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Edit leg
      </Button>
      <Modal opened={opened} onClose={close} title="Edit this leg">
        <Stack>
          <UntrackedLegFields draft={draft} onChange={setDraft} />
          {error && <Alert color="red">{error}</Alert>}
          {needsLoginState.needsLogin && <LoginControl underline="always">Log in to edit this leg</LoginControl>}
          <Button onClick={() => void handleSave()} disabled={request === null} loading={submitting}>
            Save leg
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
