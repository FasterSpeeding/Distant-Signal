'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { LoginLink } from './LoginLink';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * The confirm button inside the modal carries `aria-label="Confirm delete"`
 * so it has a distinct accessible name from this component's own trigger
 * button once both are simultaneously in the DOM (both read "Delete" as
 * their visible text, matching typical confirm-dialog UX).
 *
 * `delete_line` requires `AuthenticatedUser` (`crates/api/src/routes/lines.rs`),
 * and `/lines/[id]/page.tsx` now only renders this button for the line's
 * real owner (see that page's `isOwner` gate). So a `401` here can, in
 * practice, only happen from a session that lapses between page load and
 * this click — the same narrow race `TicketPanel`'s design already
 * reasoned about (Decision 4,
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md).
 * Matches `PinToggle`'s established `needsLogin` pattern: catch the `401`
 * specifically and show a login prompt, never the raw backend rejection
 * text this used to fall through to. */
export function DeleteLineButton({ id }: { id: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Set on a 401 from the delete request, cleared at the start of every
  // fresh attempt — same shape as `PinToggle`'s `needsLogin`.
  const [needsLogin, setNeedsLogin] = useState(false);

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    setNeedsLogin(false);
    try {
      const response = await fetch(`/api/lines/${id}`, { method: 'DELETE' });
      if (!response.ok) {
        // A 401's body is the backend's plain-text rejection -- never
        // shown to the user as-is (see this component's own doc comment).
        // Every other non-ok status still falls through to the generic
        // error text, unchanged from before.
        if (response.status === 401) {
          setNeedsLogin(true);
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.push('/lines');
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
      <Modal opened={opened} onClose={close} title="Delete this line?">
        <Text>This cannot be undone.</Text>
        {error && <Text c="red">{error}</Text>}
        {needsLogin && (
          <LoginLink underline="always">
            Log in to delete a line
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
