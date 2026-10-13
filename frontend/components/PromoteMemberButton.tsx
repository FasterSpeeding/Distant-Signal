'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** `owner`-only "Promote to admin" control. It confirms first, like
 * `DemoteMemberButton`: the two sit side by side in a list of member rows,
 * and a misclick on the wrong row hands someone the invite-link, rename and
 * member-removal powers with no notice to anyone. Promotion is reversible
 * (demote puts them back), so the confirm is `tone="neutral"`, not red.
 * The page refreshes on success, and the row swaps to "Demote to member". */
export function PromoteMemberButton({ groupId, userId, name }: { groupId: string; userId: string; name: string }) {
  return (
    <ConfirmActionButton
      label="Promote to admin"
      confirmLabel="Promote"
      actionName="promote member"
      title={`Promote ${name} to admin?`}
      tone="neutral"
      request={() => fetch(`/api/groups/${groupId}/members/${userId}/promote`, { method: 'POST' })}
      failure={['change', "this member's role"]}
      loginPrompt="Log in to promote this member"
    >
      They&apos;ll be able to manage the invite link, rename the group and remove other members. You can demote them
      again later.
    </ConfirmActionButton>
  );
}
