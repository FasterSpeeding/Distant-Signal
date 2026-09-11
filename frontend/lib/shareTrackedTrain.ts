'use client';

/** Best-effort follow-up shared by both "track this train" entry points
 * (`TrackThisTrainButton.tsx`, `TrackTrainForm.tsx`): shares a
 * just-created tracked train into `groupId` via
 * `POST /api/groups/{groupId}/trains`
 * (`crates/api/src/routes/groups.rs::add_group_train`, same-origin proxy to
 * `POST /public/groups/{id}/trains` -- any current member may add their OWN
 * tracked train, which the just-tracked one always is at this point).
 *
 * Called only after the track call itself has already succeeded, exactly
 * the same posture both components already apply to their existing
 * ticket-attach follow-up (see `TrackThisTrainButton.tsx`'s own doc
 * comment): the train is tracked either way by the time this runs, so
 * every failure mode here -- a network blip, a 403/404/500 -- is swallowed
 * rather than surfaced, and must never block navigation or read as the
 * track itself having failed. Mirrors that follow-up's `try {} catch {}`
 * shape exactly, just factored out since this one has no per-call-site
 * variation to keep separate (both entry points share the identical
 * request shape, unlike the track call itself). */
export async function shareTrackedTrainToGroup(groupId: string, trackingId: number): Promise<void> {
  try {
    await fetch(`/api/groups/${groupId}/trains`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ trainSubscriptionId: trackingId }),
    });
  } catch {
    // Deliberately swallowed -- see this function's own doc comment.
  }
}
