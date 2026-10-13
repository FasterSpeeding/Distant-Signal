'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Self-removal, always allowed for any member (spec §3) -- unlike
 * `RemoveMemberButton`, this targets the CURRENT user's own id, so it
 * needs no `admin`/`owner` gating at the call site: the page renders this
 * for every member's own row unconditionally. On success, navigates back
 * to `/groups` (there's no reason to stay on a group's own detail page
 * once you've left it) -- same "navigate away from a now-gone-to-you
 * resource" reasoning as `DeleteTrainButton`'s own redirect.
 *
 * `willDeleteGroup` should be `true` when the caller is the sole owner of
 * an otherwise-empty group: `groups::remove_member`'s sole-owner-leaves
 * branch deletes the WHOLE group in that case, rather than leaving the
 * caller's own membership row removed -- the confirm copy below says so
 * explicitly instead of the generic "you'll lose access" wording, so this
 * doesn't read as a smaller action than it is.
 *
 * `nextOwnerLabel` (review §3.2.1, design decision) names who inherits
 * ownership when the caller is an owner leaving a group that ISN'T sole
 * (`willDeleteGroup` false): `remove_member`'s owner branch
 * (`crates/api/src/data/groups.rs`) always transfers ownership to the
 * longest-standing remaining admin, or failing that the longest-standing
 * remaining member, rather than leaving the group ownerless. That transfer
 * is the one consequence of leaving a departing owner would not otherwise
 * guess -- every other member just sees the group carry on -- so it gets
 * its own explicit sentence rather than being folded silently into the
 * generic "you'll lose access" copy every other leaving member sees. `null`
 * (the default) renders the generic copy unchanged, which covers every
 * non-owner caller and the sole-owner-deletes-the-group case above. */
export function LeaveGroupButton({
  groupId,
  currentUserId,
  willDeleteGroup = false,
  nextOwnerLabel = null,
}: {
  groupId: string;
  currentUserId: string;
  willDeleteGroup?: boolean;
  nextOwnerLabel?: string | null;
}) {
  return (
    <ConfirmActionButton
      label="Leave group"
      confirmLabel="Leave"
      actionName="leave group"
      title="Leave this group?"
      request={() => fetch(`/api/groups/${groupId}/members/${currentUserId}`, { method: 'DELETE' })}
      failure={['leave', 'this group']}
      loginPrompt="Log in to leave this group"
      onSuccess={(router) => router.push('/groups')}
    >
      {willDeleteGroup
        ? "You're the only member, so leaving deletes this group, its shared trains and its invite link."
        : `You'll lose access to every train shared in this group, and any trains you've shared into it will be removed for everyone else too.${nextOwnerLabel ? ` ${nextOwnerLabel} will become the new owner.` : ''}`}
    </ConfirmActionButton>
  );
}
