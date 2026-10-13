'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Removes a shared journey from a group -- the sharer, or any `admin`/
 * `owner`, may click this (the backend enforces which via
 * `groups::remove_journey_from_group`'s sharer-or-manager check). Mirrors
 * `RemoveGroupTrainButton.tsx` verbatim -- see that component's own doc
 * comment for the full reasoning, which applies here unchanged. */
export function RemoveGroupJourneyButton({ groupId, journeyId }: { groupId: string; journeyId: number }) {
  return (
    <ConfirmActionButton
      label="Remove from group"
      confirmLabel="Remove"
      actionName="remove journey from group"
      title="Remove this journey from the group?"
      request={() => fetch(`/api/groups/${groupId}/journeys/${journeyId}`, { method: 'DELETE' })}
      failure={['remove', 'this journey from the group']}
      loginPrompt="Log in to remove this journey"
    />
  );
}
