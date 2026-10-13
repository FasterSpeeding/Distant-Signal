'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** `owner`-only "Demote to member" control, the inverse of
 * `PromoteMemberButton` and gated the same way (the backend's
 * `demote_member` handler checks `GroupRole::is_owner`, not `can_manage`,
 * so an admin who clicked this would only ever get a 403).
 *
 * Both directions confirm, through `ConfirmActionButton` with
 * `tone="neutral"`: a role change is reversible but silent for the member,
 * and the buttons sit one row apart from "Remove" in a list where a
 * misclick lands on the wrong person. The member keeps their membership and
 * everything they've shared into the group, and the confirm copy says so,
 * so this doesn't read as a removal. */
export function DemoteMemberButton({ groupId, userId, name }: { groupId: string; userId: string; name: string }) {
  return (
    <ConfirmActionButton
      label="Demote to member"
      confirmLabel="Demote"
      actionName="demote member"
      title={`Demote ${name} to member?`}
      tone="neutral"
      request={() => fetch(`/api/groups/${groupId}/members/${userId}/demote`, { method: 'POST' })}
      failure={['change', "this member's role"]}
      loginPrompt="Log in to demote this member"
    >
      They&apos;ll stay in the group and keep everything they&apos;ve shared into it, but they&apos;ll no longer be able
      to manage the invite link, rename the group, or remove other members.
    </ConfirmActionButton>
  );
}
