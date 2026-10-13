'use client';

import { forwardRef } from 'react';
import { Text } from '@mantine/core';
import { ConfirmActionButton, type ConfirmActionButtonHandle } from './ConfirmActionButton';

/** Imperative escape hatch for `TrackedTrainRowMenu`: see `hideTrigger`
 * below. */
export type DeleteTrainButtonHandle = ConfirmActionButtonHandle;

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * `/api/Train/{trackingId}` is passed straight through to the backend's
 * `DELETE /Train/{trackingId}` (`crates/api/src/routes/train.rs::delete_tracked_train`)
 * with no `/public/` prefix inserted -- see that proxy's own
 * `resolveTargetPath` comment for why `Train/...` requests are special-cased.
 * The confirm step is `ConfirmActionButton`'s.
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
 * `hideTrigger` renders no button at all, and `ref` exposes `{ open }`
 * (`ConfirmActionButton`'s handle): `/track/mine`'s row offers this as a
 * `Menu.Item` in an overflow kebab (Task 3.6.7) rather than a second
 * free-standing button. `TrackedTrainRowMenu` renders this component as a
 * sibling of `<Menu>`, never inside `<Menu.Dropdown>`: Mantine's `Popover`
 * closes on the same click that opens the modal and hid a nested modal's
 * portal wrapper with `display: none !important` (traced live), so the
 * menu item calls `ref.current.open()` instead. */
export const DeleteTrainButton = forwardRef<
  DeleteTrainButtonHandle,
  {
    trackingId: number;
    sharedGroupCount: number;
    afterDelete?: 'redirect' | 'refresh' | undefined;
    hideTrigger?: boolean | undefined;
  }
>(function DeleteTrainButton({ trackingId, sharedGroupCount, afterDelete = 'redirect', hideTrigger = false }, ref) {
  return (
    <ConfirmActionButton
      ref={ref}
      hideTrigger={hideTrigger}
      label="Stop tracking"
      actionName="stop tracking"
      title="Stop tracking this train?"
      request={() => fetch(`/api/Train/${trackingId}`, { method: 'DELETE' })}
      failure={['remove', 'this train']}
      loginPrompt="Log in to stop tracking this train"
      onSuccess={(router) => (afterDelete === 'refresh' ? router.refresh() : router.push('/track/mine'))}
    >
      <Text>This cannot be undone.</Text>
      {sharedGroupCount > 0 && (
        <Text>
          This train is shared in {sharedGroupCount} group{sharedGroupCount === 1 ? '' : 's'}. Deleting it will remove
          it from {sharedGroupCount === 1 ? 'that group' : 'those groups'} too.
        </Text>
      )}
    </ConfirmActionButton>
  );
});
