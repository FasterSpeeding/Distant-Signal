'use client';

import { useState } from 'react';
import { Button, Modal, Select } from '@mantine/core';
import type { GroupSummary } from '@/lib/types';

/** Not a real group id -- `Select` needs some string value for the
 * "Personal" option, and a real `groups.id` is a 32-byte base64url token
 * (`auth::generate_session_token()`), so this can never collide with one. */
const PERSONAL_VALUE = '__personal__';

/** The "Personal, or one of your groups?" destination prompt shown in front
 * of BOTH "track this train" entry points once the signed-in user is a
 * member of at least one group -- `TrackThisTrainButton.tsx` and
 * `TrackTrainForm.tsx` are the only two call sites, each deciding whether to
 * render this at all from their own `useGroupSummaries()` result (an empty
 * list means neither ever renders it, preserving today's exact zero-groups
 * behavior).
 *
 * Deliberately closes itself the instant `onConfirm` fires, BEFORE the
 * actual track request even starts, by calling `onClose` first: the calling
 * component's own existing busy/error/login-prompt handling (already fully
 * built and tested for the zero-groups flow) takes over from there exactly
 * as it does today, so this component owns nothing about the track
 * request's outcome -- only which destination was picked. That keeps the
 * has-groups path a thin wrapper around the existing flow rather than a
 * second, divergent one.
 *
 * "Personal" is always first and the default selection, per the feature's
 * own "should probably be the default/first option" call -- the common
 * personal-tracking case stays a single extra click (this dialog's own
 * already-selected default) away. */
export function TrackDestinationModal({
  opened,
  groups,
  onClose,
  onConfirm,
}: {
  opened: boolean;
  groups: GroupSummary[];
  onClose: () => void;
  onConfirm: (groupId: string | null) => void;
}) {
  const [selected, setSelected] = useState<string>(PERSONAL_VALUE);

  const data = [
    { value: PERSONAL_VALUE, label: 'Personal' },
    ...groups.map((group) => ({ value: group.id, label: group.name })),
  ];

  function confirm() {
    const groupId = selected === PERSONAL_VALUE ? null : selected;
    onClose();
    onConfirm(groupId);
  }

  return (
    <Modal opened={opened} onClose={onClose} title="Track this train">
      <Select
        label="Track into"
        data={data}
        value={selected}
        onChange={(value) => setSelected(value ?? PERSONAL_VALUE)}
        allowDeselect={false}
      />
      {/* Deliberately not labelled "Track this train" -- the trigger
          button/form submit behind this modal already carries that exact
          label (unchanged from before this feature existed), and reusing it
          here would give the page two same-named buttons at once, ambiguous
          both for a screen reader and for tests (`AddTrainToGroupButton`'s
          own trigger/confirm pair -- "Add one of my trains" /
          "Add to group" -- makes the same deliberate choice). */}
      <Button mt="md" onClick={confirm}>
        Confirm
      </Button>
    </Modal>
  );
}
