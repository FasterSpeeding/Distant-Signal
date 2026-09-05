'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, TextInput, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Renames or clears a ticket's `customName` -- identical shape to
 * `RenameTrainButton.tsx` (see that component's own doc comment for the
 * full rationale, including why Save is disabled on an empty trimmed
 * input), against `POST /Train/tickets/{ticketId}/name`
 * (`crates/api/src/routes/train.rs::post_ticket_name`) instead. Two
 * separate components rather than one generic `RenameButton`, mirroring
 * this codebase's own `DeleteTrainButton`/`DeleteTicketButton` precedent. */
export function RenameTicketButton({
  ticketId,
  customName,
  defaultName,
}: {
  ticketId: number;
  customName: string | null;
  defaultName: string;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [value, setValue] = useState(customName ?? '');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setValue(customName ?? '');
    setError(null);
    open();
  }

  async function submit(nextCustomName: string | null) {
    setSaving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Train/tickets/${ticketId}/name`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: nextCustomName }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSaving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setSaving(false);
    }
  }

  const trimmed = value.trim();

  return (
    <>
      <Button variant="subtle" size="xs" onClick={handleOpen}>
        Rename
      </Button>
      <Modal opened={opened} onClose={close} title="Rename this ticket">
        <TextInput
          label="Custom name"
          placeholder={defaultName}
          value={value}
          onChange={(event) => setValue(event.currentTarget.value)}
          maxLength={200}
          data-autofocus
        />
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to rename this ticket</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={saving}>
            Cancel
          </Button>
          {customName !== null && (
            <Button variant="outline" color="red" onClick={() => submit(null)} loading={saving}>
              Clear
            </Button>
          )}
          <Button onClick={() => submit(trimmed)} loading={saving} disabled={trimmed.length === 0}>
            Save
          </Button>
        </Group>
      </Modal>
    </>
  );
}
