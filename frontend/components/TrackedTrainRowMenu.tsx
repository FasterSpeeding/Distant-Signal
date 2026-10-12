'use client';

import { useRef } from 'react';
import { ActionIcon, Menu } from '@mantine/core';
import { KebabIcon } from './KebabIcon';
import { RenameTrainButton, type RenameTrainButtonHandle } from './RenameTrainButton';
import { DeleteTrainButton, type DeleteTrainButtonHandle } from './DeleteTrainButton';

/** Task 3.6.7's `/track/mine` row overflow-kebab, extracted into its own
 * Client Component rather than inlined in `app/track/mine/page.tsx`
 * (a Server Component): `RenameTrainButton`/`DeleteTrainButton`'s `trigger`
 * prop is a function that returns JSX (`(onClick) => <Menu.Item .../>`),
 * and a function value can never cross the Server->Client boundary as a
 * prop -- only the two buttons' OTHER props (`trackingId`, `customName`,
 * etc.) are serializable. Building the `trigger` closures here, entirely
 * client-side, keeps every prop this component itself takes a plain
 * serializable value.
 *
 * `RenameTrainButton`/`DeleteTrainButton` are rendered OUTSIDE `<Menu>`
 * entirely (as siblings below it), controlled via `ref` rather than the
 * `trigger` prop each still accepts -- the `Menu.Item`s inside
 * `<Menu.Dropdown>` are plain, local, and just call `.current?.open()`.
 *
 * This is a deliberate fix, not the original shape: `trigger` alone (both
 * buttons rendering their whole selves, confirm `<Modal>` included, as
 * `Menu.Dropdown` children) reliably lost the confirm dialog moments after
 * opening it, confirmed two different ways. Without `keepMounted` on
 * `<Menu>`, Mantine's `Popover` unmounts `Menu.Dropdown`'s children once its
 * close transition finishes -- and `closeOnItemClick` (the `Menu` default)
 * closes it on the very click whose `onClick` just opened the modal, so the
 * component holding that modal's `opened` state gets torn down `~150-250ms`
 * later, taking the just-opened dialog with it. Adding `keepMounted` (the
 * obvious next fix) traded that for a different failure with the same
 * symptom: a throwaway script dumping the shared portal node's DOM showed
 * the confirm modal's own portal wrapper picking up `display: none
 * !important` at the same moment the `Menu` closes -- again on the very
 * click that opens it. Moving the stateful components out of
 * `Menu.Dropdown`'s subtree sidesteps the interaction rather than chasing
 * it further into Mantine's `Popover` internals. */
export function TrackedTrainRowMenu({
  displayName,
  trackingId,
  customName,
  defaultName,
  sharedGroupCount,
}: {
  displayName: string;
  trackingId: number;
  customName: string | null;
  defaultName: string;
  sharedGroupCount: number;
}) {
  const renameRef = useRef<RenameTrainButtonHandle>(null);
  const deleteRef = useRef<DeleteTrainButtonHandle>(null);

  return (
    <>
      <Menu position="bottom-end" withinPortal>
        <Menu.Target>
          <ActionIcon variant="subtle" color="gray" aria-label={`More actions for ${displayName}`}>
            <KebabIcon />
          </ActionIcon>
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Item onClick={() => renameRef.current?.open()}>Rename</Menu.Item>
          <Menu.Item color="red" onClick={() => deleteRef.current?.open()}>
            Stop tracking
          </Menu.Item>
        </Menu.Dropdown>
      </Menu>
      <RenameTrainButton
        ref={renameRef}
        trackingId={trackingId}
        customName={customName}
        defaultName={defaultName}
        trigger={() => null}
      />
      {/* `afterDelete="refresh"`, not the component's own 'redirect'
          default: this row's own page IS `/track/mine` already, so a
          stopped-tracking train should just drop out of this same list on
          `router.refresh()`, not navigate to the page it's already on. */}
      <DeleteTrainButton
        ref={deleteRef}
        trackingId={trackingId}
        sharedGroupCount={sharedGroupCount}
        afterDelete="refresh"
        trigger={() => null}
      />
    </>
  );
}
