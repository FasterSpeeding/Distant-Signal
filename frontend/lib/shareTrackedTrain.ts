'use client';

/** Best-effort follow-up shared by both "track this train" entry points
 * (`TrackThisTrainButton.tsx`, `TrackTrainForm.tsx`): shares the train's
 * just-created journey into `groupId` via `POST /api/groups/{groupId}/journeys`
 * (`crates/api/src/routes/groups.rs::add_group_journey`, same-origin proxy
 * to `POST /public/groups/{id}/journeys` -- any current member may add
 * their OWN journey, which the just-created one always is).
 *
 * A tracked train is a one-leg journey (DS-124), so sharing it shares the
 * journey: members see it with the group's journeys and open it at
 * `/journeys/{id}`. It used to share the train's `train_subscriptions` row
 * (`POST .../trains`), which the backend still accepts and now turns into
 * the same journey share.
 *
 * Called only after the track call itself has already succeeded, exactly
 * the same posture both components already apply to their existing
 * ticket-attach follow-up (see `TrackThisTrainButton.tsx`'s own doc
 * comment): the train is tracked either way by the time this runs, so
 * every failure mode here -- a network blip, a 403/404/500 -- is swallowed
 * rather than surfaced, and must never block navigation or read as the
 * track itself having failed. */
export async function shareTrackedTrainToGroup(groupId: string, journeyId: number): Promise<void> {
  try {
    // `encodeURIComponent`, for the same reason every path segment in
    // `lib/api.ts` has it (see that file's "Path-segment encoding invariant"
    // note): an unencoded segment lets a `../`-bearing id resolve the request
    // away from the intended proxy route.
    await fetch(`/api/groups/${encodeURIComponent(groupId)}/journeys`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ journeyId }),
    });
  } catch {
    // Deliberately swallowed -- see this function's own doc comment.
  }
}
