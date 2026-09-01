'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * `/api/Train/{trackingId}` is passed straight through to the backend's
 * `DELETE /Train/{trackingId}` (`crates/api/src/routes/train.rs::delete_tracked_train`)
 * with no `/public/` prefix inserted -- see that proxy's own
 * `resolveTargetPath` comment for why `Train/...` requests are special-cased.
 * The confirm button inside the modal carries `aria-label="Confirm delete"`
 * so it has a distinct accessible name from this component's own trigger
 * button once both are simultaneously in the DOM (both read "Delete" as
 * their visible text) -- closely modeled on `DeleteLineButton`.
 *
 * On success, redirects to `/track/mine` -- unlike a deleted custom line
 * (which returns to `/lines`, a list every line still on it belongs on),
 * there is no single "all trains" page a deleted tracked train's detail
 * page could sensibly return to; `/track/mine`, the logged-in caller's own
 * tracked-trains list, is the closest equivalent.
 *
 * `delete_tracked_train` requires `AuthenticatedUser` and 404s "doesn't
 * exist" and "exists but not yours" identically (never `403` -- see that
 * handler's own doc comment). Both train detail pages only ever render
 * this button once they already have the tracked train's state in hand
 * (i.e. never inside their own 401/not-found branches), so in practice a
 * `401` here can only happen from a session that lapses between page load
 * and this click -- the same narrow race `DeleteLineButton` already
 * reasoned about. Matches `PinToggle`'s established `needsLogin` pattern:
 * catch the `401` specifically and show a login prompt, never the raw
 * backend rejection text. */
export function DeleteTrainButton({ trackingId }: { trackingId: number }) {
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
      const response = await fetch(`/api/Train/${trackingId}`, { method: 'DELETE' });
      if (!response.ok) {
        // A 401's body is the backend's plain-text rejection -- never
        // shown to the user as-is (see this component's own doc comment).
        // Every other non-ok status still falls through to the generic
        // error text, unchanged from before.
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.push('/track/mine');
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
      <Modal opened={opened} onClose={close} title="Stop tracking this train?">
        <Text>This cannot be undone.</Text>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">
            Log in to delete this tracked train
          </LoginLink>
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
