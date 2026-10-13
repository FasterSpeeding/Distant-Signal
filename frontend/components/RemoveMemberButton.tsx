'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** `admin`/`owner`-only "Remove" control for one row in the members list,
 * via the same-origin `/api/*` proxy. Mirrors `DeleteTrainButton.tsx`'s
 * confirm-modal shape exactly. The backend's own `remove_member` handler
 * already refuses to target the `owner` row (`403`) regardless of what
 * this button does, but the caller (Task 12's page) additionally never
 * RENDERS this button for the owner's own row at all -- defense in depth,
 * not reliance on the backend alone. */
export function RemoveMemberButton({ groupId, userId, name }: { groupId: string; userId: string; name: string }) {
  return (
    <ConfirmActionButton
      label="Remove"
      actionName="remove member"
      title={`Remove ${name} from this group?`}
      request={() => fetch(`/api/groups/${groupId}/members/${userId}`, { method: 'DELETE' })}
      failure={['remove', 'this member']}
      loginPrompt="Log in to remove this member"
    />
  );
}
