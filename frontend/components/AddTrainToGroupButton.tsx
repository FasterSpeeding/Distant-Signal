'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Select, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { TrackedTrainListItem } from '@/lib/types';

/** Picker sourced from the user's own `/track/mine` list (spec §6),
 * fetched lazily on open via the same-origin `/api/Train/mine` proxy
 * (already wired -- `Train/...` requests pass through bare, no
 * `/public/` prefix, per `app/api/[...path]/route.ts`'s own
 * `resolveTargetPath`). `excludeTrainSubscriptionIds` hides trains
 * already shared into this group -- re-adding one is harmless
 * (`groups::add_train_to_group` is idempotent) but offering it again in
 * the picker would be confusing. */
export function AddTrainToGroupButton({
  groupId,
  excludeTrainSubscriptionIds,
}: {
  groupId: string;
  excludeTrainSubscriptionIds: number[];
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [loading, setLoading] = useState(false);
  const [trains, setTrains] = useState<TrackedTrainListItem[] | null>(null);
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
      const response = await fetch('/api/Train/mine');
      if (!response.ok) {
        setError('Could not load your tracked trains.');
        setLoading(false);
        return;
      }
      const all: TrackedTrainListItem[] = await response.json();
      setTrains(all.filter((t) => !excludeTrainSubscriptionIds.includes(t.id)));
      setLoading(false);
    } catch {
      setError('Could not load your tracked trains.');
      setLoading(false);
    }
  }

  async function handleAdd() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/trains`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trainSubscriptionId: Number(selected) }),
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
        Add one of my trains
      </Button>
      <Modal opened={opened} onClose={close} title="Share a tracked train with this group">
        {loading && <Text c="dimmed">Loading your tracked trains…</Text>}
        {!loading && trains !== null && trains.length === 0 && (
          <Text c="dimmed">Every train you&apos;re tracking is already shared into this group.</Text>
        )}
        {!loading && trains !== null && trains.length > 0 && (
          <Select
            label="Tracked train"
            placeholder="Pick one"
            data={trains.map((t) => ({ value: String(t.id), label: trackedTrainDisplayName(t) }))}
            value={selected}
            onChange={setSelected}
          />
        )}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share a train</LoginLink>}
        <Button mt="md" onClick={handleAdd} disabled={!selected} loading={submitting}>
          Add to group
        </Button>
      </Modal>
    </>
  );
}
