'use client';

import { ActionIcon, Menu } from '@mantine/core';
import { KebabIcon } from './KebabIcon';
import { RenameTrainButton } from './RenameTrainButton';
import { DeleteTrainButton } from './DeleteTrainButton';

/** Task 3.6.7's `/track/mine` row overflow-kebab, extracted into its own
 * Client Component rather than inlined in `app/track/mine/page.tsx`
 * (a Server Component): `RenameTrainButton`/`DeleteTrainButton`'s `trigger`
 * prop is a function that returns JSX (`(onClick) => <Menu.Item .../>`),
 * and a function value can never cross the Server->Client boundary as a
 * prop -- only the two buttons' OTHER props (`trackingId`, `customName`,
 * etc.) are serializable. Building the `trigger` closures here, entirely
 * client-side, keeps every prop this component itself takes a plain
 * serializable value. */
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
  return (
    <Menu position="bottom-end" withinPortal>
      <Menu.Target>
        <ActionIcon variant="subtle" color="gray" aria-label={`More actions for ${displayName}`}>
          <KebabIcon />
        </ActionIcon>
      </Menu.Target>
      <Menu.Dropdown>
        <RenameTrainButton
          trackingId={trackingId}
          customName={customName}
          defaultName={defaultName}
          trigger={(onClick) => <Menu.Item onClick={onClick}>Rename</Menu.Item>}
        />
        {/* `afterDelete="refresh"`, not the component's own 'redirect'
            default: this row's own page IS `/track/mine` already, so a
            stopped-tracking train should just drop out of this same list on
            `router.refresh()`, not navigate to the page it's already on. */}
        <DeleteTrainButton
          trackingId={trackingId}
          sharedGroupCount={sharedGroupCount}
          afterDelete="refresh"
          trigger={(onClick) => (
            <Menu.Item color="red" onClick={onClick}>
              Stop tracking
            </Menu.Item>
          )}
        />
      </Menu.Dropdown>
    </Menu>
  );
}
