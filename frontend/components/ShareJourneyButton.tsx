'use client';

import { useState } from 'react';
import { Alert, Button, Modal, Select } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { useGroupSummaries } from '@/lib/useGroupSummaries';

/** The journey-detail-page share control -- the direct analogue of
 * `AddToGroupButton.tsx` (tracked trains), one level up: starts from a
 * fixed `journeyId` and picks a `groupId` from the viewer's own groups,
 * rather than the other direction `AddJourneyToGroupButton.tsx` takes.
 * Lets an already-created journey be shared into a group after the fact
 * (including a second one) from the journey's own detail page, per spec
 * §4's "a share-to-group button" in the journey view header.
 *
 * Sourced from `useGroupSummaries()`, same as `AddToGroupButton.tsx` --
 * renders nothing at all for a viewer in zero groups (an anonymous
 * visitor, a genuinely group-less user, or a failed fetch, all treated
 * identically), so it adds no visible clutter for the common case.
 *
 * The POST here is NOT best-effort (unlike whatever journey-creation-time
 * group-share flow Phase 1's `TrackTrainForm`-equivalent may have) --
 * sharing IS the entire point of this click, so a failure must be shown,
 * matching `AddToGroupButton.tsx`'s own not-best-effort posture for its
 * own (also after-the-fact) share action. */
export function ShareJourneyButton({ journeyId }: { journeyId: number }) {
  const { groups } = useGroupSummaries();
  const [opened, { open, close }] = useDisclosure(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addedGroupName, setAddedGroupName] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  if (groups.length === 0) {
    return null;
  }

  function handleOpen() {
    setSelected(null);
    setError(null);
    setAddedGroupName(null);
    open();
  }

  async function handleShare() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    setAddedGroupName(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${selected}/journeys`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ journeyId }),
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
      const group = groups.find((g) => g.id === selected);
      setAddedGroupName(group?.name ?? 'the group');
      setSelected(null);
      setSubmitting(false);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Share with a group
      </Button>
      <Modal opened={opened} onClose={close} title="Share this journey with a group">
        <Select
          label="Group"
          placeholder="Pick one"
          data={groups.map((group) => ({ value: group.id, label: group.name }))}
          value={selected}
          onChange={setSelected}
        />
        {addedGroupName && <Alert color="green">Added to {addedGroupName}.</Alert>}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share this journey</LoginLink>}
        <Button mt="md" onClick={handleShare} disabled={!selected} loading={submitting}>
          Share
        </Button>
      </Modal>
    </>
  );
}
