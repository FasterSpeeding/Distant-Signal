'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * -- this is a Client Component and cannot reach the `api` service directly.
 * `/api/Train/tickets/{ticketId}` is passed straight through to the
 * backend's `DELETE /Train/tickets/{ticketId}`
 * (`crates/api/src/routes/train.rs::delete_ticket`) with no `/public/`
 * prefix inserted -- see that proxy's own `resolveTargetPath` comment.
 * Closely modeled on `DeleteTrainButton.tsx` (same confirm-modal shape,
 * same distinct `aria-label="Confirm delete"` naming rationale for the two
 * same-text "Delete" buttons, same `useNeedsLogin`/`LoginLink` `401`
 * handling, same generic-error-message fallback for any other non-`ok`
 * status) -- but calls `router.refresh()` on success, never
 * `router.push(...)`.
 *
 * This is a deliberate divergence from `DeleteTrainButton`: deleting a
 * tracked train removes the entire subject of the page it's rendered on,
 * so navigating away is correct there. Deleting a *ticket* always happens
 * from inside a list of other things (a train's other tickets, or
 * `/track/mine`'s other trains/tickets) that remain valid and worth
 * showing afterwards -- `router.refresh()` is the same mechanism
 * `AttachTicketAction.tsx:41` and `PinToggle.tsx:99` already use for this
 * "mutate one row, stay on this page" shape. It re-runs the enclosing
 * Server Component (`TicketPanel`, or `app/track/mine/page.tsx`), which
 * naturally drops the now-deleted ticket from its next render -- no
 * client-side list-splicing logic needed here.
 *
 * A `401` here can only really happen from a session that lapses between
 * page load and this click (every call site only ever renders this button
 * for a ticket the enclosing Server Component just fetched) -- same narrow
 * race `DeleteTrainButton` already reasons about. A `404` (a
 * double-click/stale-render race) is not treated as a distinguishable case
 * either, same posture.
 *
 * Known, accepted trade-off shared with every other per-row action in this
 * app (e.g. `PinToggle` on a list page): this component is rendered once
 * per ticket, each instance with its own independent `opened`/`deleting`
 * state, so nothing prevents a caller from having two different tickets'
 * confirm modals open at once. Not a new risk this component introduces --
 * no per-row action in this codebase guards against that today. */
export function DeleteTicketButton({ ticketId }: { ticketId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Train/tickets/${ticketId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Delete
      </Button>
      <Modal opened={opened} onClose={close} title="Delete this ticket?">
        <Text>This cannot be undone.</Text>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to delete this ticket</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button color="red" onClick={handleDelete} loading={deleting} aria-label="Confirm delete">
            Delete
          </Button>
        </Group>
      </Modal>
    </>
  );
}
