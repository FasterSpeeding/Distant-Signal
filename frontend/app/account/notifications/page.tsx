import { Stack, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllLines, getPreferences, getSessionOrLoggedOut } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';
import { AccountSettingsLoginPrompt } from '../preferences/AccountSettingsLoginPrompt';
import { NotificationSettings, type PinnedLineAlert } from './NotificationSettings';

// Session-dependent: never prerender (see app/page.tsx's own comment).
export const revalidate = 0;

export const metadata: Metadata = {
  ...pageMetadata('Notifications', 'Choose which notifications you get, when you get them and on which devices.'),
  robots: { index: false },
};

/** `/account/notifications` (DS-153): every notification setting on one
 * page, linked from the Preferences page's Notifications section. The
 * settings themselves load in the browser (`NotificationSettings`); this
 * only reads the pinned lines, with their names, for the line switches.
 * Logged out, the one log in prompt the Preferences page uses. */
export default async function NotificationsPage() {
  const session = await getSessionOrLoggedOut();
  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="lg" maw={640}>
        <Title order={1}>Notifications</Title>
        <AccountSettingsLoginPrompt />
      </Stack>
    );
  }
  // Each read degrades on its own: no line names falls back to the ids.
  const [preferences, lines] = await Promise.all([getPreferences().catch(() => null), getAllLines().catch(() => [])]);
  const names = new Map(lines.map((line) => [line.id, line.name]));
  const silent = new Set(preferences?.silentPinnedLines ?? []);
  const pinnedLines: PinnedLineAlert[] = (preferences?.pinnedLines ?? []).map((id) => ({
    id,
    name: names.get(id) ?? id,
    alerts: !silent.has(id),
  }));

  return (
    <Stack p="lg" gap="lg" maw={640}>
      <Title order={1}>Notifications</Title>
      <NotificationSettings pinnedLines={pinnedLines} />
    </Stack>
  );
}
