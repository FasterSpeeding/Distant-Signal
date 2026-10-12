'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Stops sharing a custom line with a group -- the member who shared it
 * (always the line's owner), or any `admin`/`owner`, may click this (the
 * backend enforces which, via `groups::remove_custom_line_grant`'s
 * sharer-or-manager check). The page
 * (`app/groups/[id]/page.tsx`'s `SharedCustomLineRow`) only renders this
 * when its own `canRemove` mirrors that same check for the current viewer
 * and row, so a plain member never sees it on someone else's shared line
 * -- defence in depth, not reliance on the backend alone, same posture as
 * `RemoveGroupTrainButton`.
 *
 * Worded "Stop sharing", not "Remove": this only revokes the group's
 * visibility. The line itself, its content and its ownership are entirely
 * untouched, and only its owner can ever delete it. */
export function RemoveCustomLineGrantButton({
  groupId,
  lineId,
  lineName,
}: {
  groupId: string;
  lineId: string;
  lineName: string;
}) {
  return (
    <ConfirmActionButton
      label="Stop sharing"
      actionName="stop sharing line"
      title="Stop sharing this line with the group?"
      request={() =>
        fetch(`/api/groups/${groupId}/lines/custom/${encodeURIComponent(lineId)}`, {
          method: 'DELETE',
        })
      }
      failure={['remove', 'this line from the group']}
      loginPrompt="Log in to stop sharing this line"
    >
      {`Group members will no longer be able to see ${lineName}. The line itself isn't deleted or changed.`}
    </ConfirmActionButton>
  );
}
