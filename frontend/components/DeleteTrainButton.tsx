'use client';

import type { ReactNode } from 'react';
import { forwardRef, useImperativeHandle, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Imperative escape hatch for `TrackedTrainRowMenu` -- see this
 * component's own `ref` doc comment below for why it exists. */
export type DeleteTrainButtonHandle = { open: () => void };

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
 * `afterDelete` controls what happens on success, and defaults to
 * `'redirect'` (push to `/track/mine`): unlike a deleted custom line (which
 * returns to `/lines`, a list every line still on it belongs on), there is
 * no single "all trains" page a deleted tracked train's detail page could
 * sensibly return to, and `/train/by-id/[trackingId]` (the by-`trackingId`
 * page, this component's original caller) is keyed by a `trackingId` that
 * is now meaningless once deleted -- `/track/mine`, the logged-in caller's
 * own tracked-trains list, is the closest equivalent. `/train/[uid]/[date]`
 * (keyed by the train's real identity, not the tracking id) passes
 * `'refresh'` instead: that URL stays perfectly meaningful post-delete, so
 * it just needs its owning Server Component to re-run (`router.refresh()`,
 * the same mechanism `RenameTrainButton`/`DeleteTicketButton` already use
 * for "mutate, stay here") so a follow-up `GET /Train/mine` drops the
 * deleted row and the page swaps back to `TrackThisTrainButton`. A plain
 * boolean/string prop, not an `onDeleted` callback: both call sites reach
 * this component through `TrackedTrainOwnerControls`, which itself is
 * rendered directly from a Server Component page, and a Server Component
 * cannot hand a Client Component a function prop.
 *
 * `sharedGroupCount` (from `TrackedTrainState`/`TrackedTrainListItem`,
 * already in hand by the time either caller renders this button -- no
 * extra fetch) drives the confirm modal's copy: deleting a tracked train
 * that's shared into one or more groups also removes it from those groups
 * for everyone else who could see it there (the DB's own
 * `group_trains.train_subscription_id ... ON DELETE CASCADE` already
 * guarantees that; this only makes sure the user isn't surprised by it).
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
 * backend rejection text.
 *
 * Task 3.6.7: relabelled "Delete" -> "Stop tracking" throughout (the
 * trigger, the modal's own confirm button, and its `aria-label`) -- users
 * don't delete a train from Network Rail, they stop tracking it; the
 * modal's own title ("Stop tracking this train?") already said so, only
 * the buttons hadn't caught up. The red outline + confirm-modal shape is
 * otherwise unchanged.
 *
 * `trigger`, when given, replaces the default `Button` -- a render prop
 * rather than a `children` override, since it needs the `onClick` that
 * opens this component's own confirm modal, not a static node. Added for
 * `/track/mine`'s list row (Task 3.6.7's own "overflow kebab" fix), which
 * renders this control as a `Menu.Item` instead of a second free-standing
 * button competing for space on an already-tight row; every other caller
 * omits it and keeps today's exact `Button`.
 *
 * `ref` exposes `{ open }` via `useImperativeHandle` -- added because
 * `trigger`'s own fix (render the `Menu.Item` in place) turned out not to
 * be enough on its own: `TrackedTrainRowMenu` renders this whole component,
 * confirm `<Modal>` included, as a REACT CHILD of `<Menu.Dropdown>`, and
 * Mantine's `Popover` (what `Menu.Dropdown` is built on) does something
 * surprising with a nested `<Modal>` there -- confirmed live, with
 * `keepMounted` on the `Menu` (the obvious first fix for the plain
 * "Popover unmounts closed content" case): opening the confirm dialog via
 * `Menu.Item`'s `onClick` still made it disappear, but this time by
 * inheriting `display: none` from the closing `Popover`, on a wrapper
 * `Popover` itself doesn't own (traced with a throwaway script dumping the
 * shared portal node's DOM: the modal's own portal wrapper picks up
 * `display: none !important` the moment the `Menu` closes, which happens on
 * the very same click that opens it). `TrackedTrainRowMenu` uses this `ref`
 * to call `open()` from a plain, local `Menu.Item` instead of using
 * `trigger` -- rendering this whole component (Modal included) as a
 * sibling of `<Menu>`, never a descendant of `<Menu.Dropdown>`, sidesteps
 * the Popover interaction entirely rather than fighting it. `trigger`
 * itself is left in place for other simpler embeddings (and is still
 * exercised by this component's own tests); `ref` is additive, not a
 * replacement. */
export const DeleteTrainButton = forwardRef<
  DeleteTrainButtonHandle,
  {
    trackingId: number;
    sharedGroupCount: number;
    afterDelete?: 'redirect' | 'refresh';
    trigger?: (onClick: () => void) => ReactNode;
  }
>(function DeleteTrainButton({ trackingId, sharedGroupCount, afterDelete = 'redirect', trigger }, ref) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  useImperativeHandle(ref, () => ({ open }), [open]);

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
      if (afterDelete === 'refresh') {
        router.refresh();
      } else {
        router.push('/track/mine');
      }
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      {trigger ? (
        trigger(open)
      ) : (
        <Button variant="outline" color="red" size="xs" onClick={open}>
          Stop tracking
        </Button>
      )}
      <Modal opened={opened} onClose={close} title="Stop tracking this train?">
        <Text>This cannot be undone.</Text>
        {sharedGroupCount > 0 && (
          <Text>
            This train is shared in {sharedGroupCount} group{sharedGroupCount === 1 ? '' : 's'} — deleting it will
            remove it from {sharedGroupCount === 1 ? 'that group' : 'those groups'} too.
          </Text>
        )}
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">
            Log in to stop tracking this train
          </LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button color="red" onClick={handleDelete} loading={deleting} aria-label="Confirm stop tracking">
            Stop tracking
          </Button>
        </Group>
      </Modal>
    </>
  );
});
