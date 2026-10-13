'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** `owner`-only "Delete group" control for `DELETE /groups/{id}` (spec §3:
 * "Delete the group entirely | `owner` only" -- the caller renders this
 * only for an owner, and the backend's `require_role(..,
 * GroupRole::is_owner)` refuses everyone else regardless).
 *
 * Mirrors `LeaveGroupButton.tsx`'s confirm-modal shape rather than a
 * bare button: `groups::delete_group`'s `ON DELETE CASCADE` takes the
 * memberships, the shared-train rows, and the invite links with it, for
 * EVERY member -- strictly more destructive than leaving, so it gets at
 * least the same confirmation weight. Navigates to `/groups` on success,
 * the same "navigate away from a now-gone resource" move
 * `DeleteTrainButton`/`LeaveGroupButton` both make; staying on a detail
 * page for a group that no longer exists would just render a 404.
 *
 * `variant="subtle"` rather than the `outline` red `LeaveGroupButton` uses
 * (review §3.2.1, design decision): the two used to be visually identical
 * -- same red outline, 12px apart -- despite "leave" only ever affecting
 * the caller and "delete" wiping the group for every member. The caller
 * (`app/groups/[id]/page.tsx`) now renders this alone in a "Danger zone"
 * section at the foot of the page rather than beside "Leave group" in the
 * header, so the demotion to a subtler style reinforces a demotion in
 * position rather than fighting it -- "Leave group" keeps the header's red
 * outline treatment since it's the control most viewers actually want. The
 * confirm modal below is unchanged: this is a visual-precedence fix, not a
 * safety one, and the two-step confirm is still exactly as hard to
 * trigger by accident as it was before. */
export function DeleteGroupButton({ groupId, name }: { groupId: string; name: string }) {
  return (
    <ConfirmActionButton
      label="Delete group"
      confirmLabel="Delete"
      actionName="delete group"
      title={`Delete ${name}?`}
      request={() => fetch(`/api/groups/${groupId}`, { method: 'DELETE' })}
      failure={['delete', 'this group']}
      loginPrompt="Log in to delete this group"
      onSuccess={(router) => router.push('/groups')}
    >
      This deletes the group for everyone. Every member loses access, every train shared into it stops being shared, and
      the invite link stops working. This can&apos;t be undone.
    </ConfirmActionButton>
  );
}
