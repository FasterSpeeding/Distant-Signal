'use client';

import { ActionIcon, Tooltip, type MantineBreakpoint } from '@mantine/core';
import Link from 'next/link';
import { SettingsIcon } from './SettingsIcon';
import { PREFERENCES_DESTINATION } from '@/lib/navLinks';

/** The gear in the nav bar that opens /account/preferences, for a
 * logged-out visitor at `md` and up (`AppNavBar` decides both). The one
 * exception to "no settings in the bar" (docs/style-guide.md, "Nav"): it is
 * a link to the settings page, not a control, and without it a logged-out
 * desktop visitor could only find their theme and pride flag in the footer.
 * Logged in, the account menu links there instead; below `md`, the drawer.
 *
 * A Client Component because `component={Link}` can't cross from the
 * Server Component bar (see `AppNavBar`'s note on the wordmark link). */
export function PreferencesNavIcon({ visibleFrom }: { visibleFrom: MantineBreakpoint }) {
  return (
    <Tooltip label={PREFERENCES_DESTINATION.label} position="bottom-end" zIndex={400}>
      {/* `size="md"` (28px), the same as the freshness icon beside it. */}
      <ActionIcon
        component={Link}
        href={PREFERENCES_DESTINATION.href}
        variant="subtle"
        size="md"
        aria-label={PREFERENCES_DESTINATION.label}
        visibleFrom={visibleFrom}
      >
        <SettingsIcon />
      </ActionIcon>
    </Tooltip>
  );
}
