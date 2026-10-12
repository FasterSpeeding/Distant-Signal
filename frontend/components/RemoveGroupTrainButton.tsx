'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Removes a shared train from a group -- the sharer, or any `admin`/
 * `owner`, may click this (the backend enforces which via
 * `groups::remove_train_from_group`'s sharer-or-manager check). The page
 * (`app/groups/[id]/page.tsx`'s `SharedTrainRow`) only renders this when
 * its own `canRemove` mirrors that same check for the current viewer and
 * row, so a plain member never sees it on someone else's shared train --
 * defense in depth, not reliance on the backend alone, same posture as
 * `RemoveMemberButton`'s owner-row gating. Mirrors `DeleteTrainButton.tsx`'s
 * confirm-modal shape. */
export function RemoveGroupTrainButton({
  groupId,
  trainSubscriptionId,
}: {
  groupId: string;
  trainSubscriptionId: number;
}) {
  return (
    <ConfirmActionButton
      label="Remove from group"
      confirmLabel="Remove"
      actionName="remove train from group"
      title="Remove this train from the group?"
      request={() => fetch(`/api/groups/${groupId}/trains/${trainSubscriptionId}`, { method: 'DELETE' })}
      failure={['remove', 'this train from the group']}
      loginPrompt="Log in to remove this train"
    />
  );
}
