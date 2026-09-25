'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Select, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { JourneyListItem } from '@/lib/types';

/** Picker sourced from the user's own `/Journeys/mine` list -- the direct
 * analogue of `AddTrainToGroupButton.tsx`, one level up (journeys instead
 * of tracked trains). Fetched lazily on open via the same-origin
 * `/api/Journeys/mine` proxy, mirroring that component's own
 * `/api/Train/mine` fetch. `excludeJourneyIds` hides journeys already
 * shared into this group -- re-adding one is harmless
 * (`groups::add_journey_to_group` is idempotent) but offering it again in
 * the picker would be confusing.
 *
 * Display name is computed inline (`j.customName ?? "<origin> → <destination>"`,
 * falling back to `'Untitled journey'` when neither CRS is set) rather than
 * via a shared `journeyDisplayName` helper -- no such module exists in this
 * codebase (`JourneyListItem`, from `lib/types.ts`, has no
 * `pinOriginName`/`pinDestinationName`, only bare CRS codes), and this is
 * the only call site that needs it. */
export function AddJourneyToGroupButton({
  groupId,
  excludeJourneyIds,
}: {
  groupId: string;
  excludeJourneyIds: number[];
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [loading, setLoading] = useState(false);
  const [journeys, setJourneys] = useState<JourneyListItem[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleOpen() {
    setError(null);
    setSelected(null);
    open();
    setLoading(true);
    try {
      const response = await fetch('/api/Journeys/mine');
      if (!response.ok) {
        setError('Could not load your journeys.');
        setLoading(false);
        return;
      }
      const all: JourneyListItem[] = await response.json();
      setJourneys(all.filter((j) => !excludeJourneyIds.includes(j.id)));
      setLoading(false);
    } catch {
      setError('Could not load your journeys.');
      setLoading(false);
    }
  }

  async function handleAdd() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/journeys`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ journeyId: Number(selected) }),
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
        Add one of my journeys
      </Button>
      <Modal opened={opened} onClose={close} title="Share a journey with this group">
        {loading && <Text c="dimmed">Loading your journeys…</Text>}
        {!loading && journeys !== null && journeys.length === 0 && (
          <Text c="dimmed">Every journey you have is already shared into this group.</Text>
        )}
        {!loading && journeys !== null && journeys.length > 0 && (
          <Select
            label="Journey"
            placeholder="Pick one"
            data={journeys.map((j) => ({
              value: String(j.id),
              label: j.customName ?? (j.originCrs && j.destinationCrs ? `${j.originCrs} → ${j.destinationCrs}` : 'Untitled journey'),
            }))}
            value={selected}
            onChange={setSelected}
          />
        )}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share a journey</LoginLink>}
        <Button mt="md" onClick={handleAdd} disabled={!selected} loading={submitting}>
          Add to group
        </Button>
      </Modal>
    </>
  );
}
