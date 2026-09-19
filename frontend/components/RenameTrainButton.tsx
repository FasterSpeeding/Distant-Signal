'use client';

import type { ReactNode } from 'react';
import { forwardRef, useImperativeHandle, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, TextInput, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Imperative escape hatch for `TrackedTrainRowMenu` -- see
 * `DeleteTrainButtonHandle` (`DeleteTrainButton.tsx`) for why this exists. */
export type RenameTrainButtonHandle = { open: () => void };

/** Renames or clears a tracked train's `customName`, via the same-origin
 * `/api/*` proxy (see `app/api/[...path]/route.ts`) -- this is a Client
 * Component and cannot reach the `api` service directly.
 * `/api/Train/{trackingId}/name` is passed straight through to the
 * backend's `POST /Train/{trackingId}/name`
 * (`crates/api/src/routes/train.rs::post_tracked_train_name`) with no
 * `/public/` prefix inserted, same as `DeleteTrainButton`.
 *
 * Closely modeled on `DeleteTrainButton.tsx`: same button → confirm-modal →
 * fetch → `router.refresh()` shape, same `useNeedsLogin`/`LoginLink` `401`
 * handling, same generic-error-message fallback for any other non-`ok`
 * status. Unlike `DeleteTrainButton`, this is never destructive in a way
 * that leaves the page's own subject gone, so it always `router.refresh()`s
 * on success rather than navigating away -- same reasoning
 * `DeleteTicketButton` already gives for its own `router.refresh()` choice.
 *
 * `Save` is disabled whenever the trimmed input is empty -- this is the
 * one deliberate divergence from a plain "submit whatever's in the box"
 * pattern (see
 * docs/superpowers/plans/2026-09-05-custom-tracking-names-plan.md's
 * Judgment Call 3): the backend already normalizes an empty-after-trim
 * value to "clear the name" on any successful write, so without this,
 * accidentally emptying the field and hitting Save would silently clear a
 * name the user meant to just edit. Disabling Save on empty input means
 * clearing only ever happens through the explicit `Clear` button (visible
 * only when a custom name is currently set), which needs no typing at all.
 *
 * `trigger`, when given, replaces the default `Button` -- see
 * `DeleteTrainButton.tsx`'s own doc comment on its matching `trigger` prop
 * for why (Task 3.6.7's `/track/mine` list-row overflow-kebab fix).
 *
 * `ref` exposes `{ open: handleOpen }` -- same escape hatch, and the same
 * reason, as `DeleteTrainButton`'s own `ref` (see its doc comment): a
 * `<Modal>` rendered as a descendant of `<Menu.Dropdown>` picks up
 * `display: none` from the surrounding `Popover` closing on the very click
 * that opens it. `TrackedTrainRowMenu` uses this to keep the whole
 * component -- Modal included -- outside `<Menu.Dropdown>`'s subtree. */
export const RenameTrainButton = forwardRef<
  RenameTrainButtonHandle,
  {
    trackingId: number;
    customName: string | null;
    defaultName: string;
    trigger?: (onClick: () => void) => ReactNode;
  }
>(function RenameTrainButton({ trackingId, customName, defaultName, trigger }, ref) {
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

  useImperativeHandle(ref, () => ({ open: handleOpen }), [customName]);

  async function submit(nextCustomName: string | null) {
    setSaving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Train/${trackingId}/name`, {
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
      {trigger ? (
        trigger(handleOpen)
      ) : (
        <Button variant="subtle" size="xs" onClick={handleOpen}>
          Rename
        </Button>
      )}
      <Modal opened={opened} onClose={close} title="Rename this tracked train">
        <TextInput
          label="Custom name"
          placeholder={defaultName}
          value={value}
          onChange={(event) => setValue(event.currentTarget.value)}
          maxLength={200}
          data-autofocus
        />
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to rename this tracked train</LoginLink>
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
});
