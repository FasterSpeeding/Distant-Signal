'use client';

import { useState } from 'react';
import { Alert, Button, Modal, Select } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { useGroupSummaries } from '@/lib/useGroupSummaries';

/** The mirror-image of `AddTrainToGroupButton.tsx`: that component starts
 * from a fixed `groupId` and picks a `trainSubscriptionId` from the
 * viewer's own `GET /Train/mine` list; this one starts from a fixed
 * `trainSubscriptionId` (a train the viewer already owns, per
 * `TrackedTrainOwnerControls`'s own ownership guarantee) and picks a
 * `groupId` from the viewer's own groups. Lets an already-tracked train be
 * shared into a group after the fact -- including a SECOND group -- without
 * deleting and re-tracking it, which `TrackDestinationModal` (the
 * track-TIME picker) has no way to do since it only ever runs once, at the
 * moment a train is first tracked.
 *
 * Sourced from `useGroupSummaries()` (`lib/useGroupSummaries.ts`) rather
 * than a fresh `GET /api/groups` fetch of its own -- that hook already
 * fetches the caller's groups once on mount and treats a zero-groups
 * result, an anonymous visitor, or a failed fetch identically as "nothing
 * to offer", which is exactly this component's own "render nothing at all"
 * requirement below.
 *
 * Unlike `shareTrackedTrainToGroup` (`lib/shareTrackedTrain.ts`), the POST
 * here is NOT best-effort: that helper exists for the track-time flow,
 * where sharing is a secondary follow-up to a track that already
 * succeeded, so a share failure there is swallowed rather than surfacing as
 * an error. Here, sharing IS the entire point of the click -- a failure
 * must be shown, not silently dropped -- so this component makes its own
 * `fetch` call and surfaces any non-`ok` response the same way
 * `AddTrainToGroupButton.tsx` already does for its own (also
 * not-best-effort) add call.
 *
 * `POST /public/groups/{id}/trains`
 * (`crates/api/src/routes/groups.rs::add_group_train`,
 * `crates/api/src/data/groups.rs::add_train_to_group`) is already
 * idempotent -- re-adding a train already in a group is a harmless no-op --
 * so this never needs to fetch a given group's existing trains first just
 * to filter the picker; every one of the viewer's groups is always a valid,
 * safe choice. On success the modal stays open (rather than closing, unlike
 * `AddTrainToGroupButton.tsx`) and shows a lightweight "Added to {group}."
 * confirmation instead: this train's detail page has no visible per-group
 * membership list for a `router.refresh()` to update, so closing silently
 * would leave the click's own outcome invisible. Staying open also means
 * sharing the same train into a SECOND group is just "pick another group,
 * click Share again" rather than a whole new interaction from scratch.
 *
 * `variant="default"`/`size="xs"`, matching `RenameTrainButton`/
 * `DeleteTrainButton` -- the third control alongside those two on
 * `TrackedTrainOwnerControls`. The trigger button's own label ("Add to
 * group") and the modal's confirm button ("Share") are deliberately
 * different strings -- same reasoning `AddTrainToGroupButton.tsx`'s own doc
 * comment gives for its "Add one of my trains" / "Add to group" pair: both
 * are simultaneously in the DOM once the modal is open, and screen readers
 * (and tests) need a distinct accessible name for each. */
export function AddToGroupButton({ trainSubscriptionId }: { trainSubscriptionId: number }) {
  const { groups } = useGroupSummaries();
  const [opened, { open, close }] = useDisclosure(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addedGroupName, setAddedGroupName] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  // Zero groups (anonymous visitor, a genuinely group-less user, or a failed
  // fetch -- `useGroupSummaries` treats all three identically) means nothing
  // to offer: no button, no empty state, per this feature's own "don't
  // clutter the common case" requirement.
  if (groups.length === 0) {
    return null;
  }

  function handleOpen() {
    setSelected(null);
    setError(null);
    setAddedGroupName(null);
    open();
  }

  async function handleShare() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    setAddedGroupName(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${selected}/trains`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trainSubscriptionId }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      const group = groups.find((g) => g.id === selected);
      setAddedGroupName(group?.name ?? 'the group');
      setSelected(null);
      setSubmitting(false);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Add to group
      </Button>
      <Modal opened={opened} onClose={close} title="Share this tracked train with a group">
        <Select
          label="Group"
          placeholder="Pick one"
          data={groups.map((group) => ({ value: group.id, label: group.name }))}
          value={selected}
          onChange={setSelected}
        />
        {addedGroupName && <Alert color="green">Added to {addedGroupName}.</Alert>}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share this train</LoginLink>}
        <Button mt="md" onClick={handleShare} disabled={!selected} loading={submitting}>
          Share
        </Button>
      </Modal>
    </>
  );
}
