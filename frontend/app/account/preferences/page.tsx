import { Stack, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getSessionOrLoggedOut } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';
import { AccountSettingsLoginPrompt } from './AccountSettingsLoginPrompt';
import { AppearanceSection } from './AppearanceSection';
import { ClaudeKeySection } from './ClaudeKeySection';
import { NotificationsSection } from './NotificationsSection';

// Session-dependent: never prerender (see app/page.tsx's own comment).
export const revalidate = 0;

export const metadata: Metadata = {
  ...pageMetadata('Preferences', 'Choose a colour theme, turn on notifications and set other options.'),
  robots: { index: false },
};

/** `/account/preferences` (round 2, item 4): the one place for settings
 * that used to be scattered. Reached from the drawer, the account menu, the
 * logged-out bar's gear and the footer (`PREFERENCES_DESTINATION` in
 * `lib/navLinks.ts`).
 *
 * Open to everyone, and never a redirect: a logged-out visitor gets the
 * device-local sections in full and one prompt to log in for the rest.
 *
 * - Device-local, saved in this browser, no account needed:
 *   `AppearanceSection` (colour theme, pride flag).
 * - Account-backed, logged in only: `NotificationsSection`,
 *   `ClaudeKeySection`. A logged-out
 *   visitor sees `AccountSettingsLoginPrompt` in their place. A new
 *   account-backed section goes in the logged-in list below and is covered
 *   by that prompt; a new device-local one goes beside `AppearanceSection`.
 *
 * Logging in keeps whatever was chosen while logged out: the account
 * stores no copy of the device-local settings, so there is nothing to
 * merge or overwrite, and logging out leaves them too (they are not in
 * `lib/browserAccountData.ts`'s cleared keys). */
export default async function PreferencesPage() {
  // Degrades rather than throws: a logged-out visitor is a session with
  // `authenticated: false`, never a 401.
  const session = await getSessionOrLoggedOut();

  return (
    <Stack p="lg" gap="lg" maw={640}>
      <Title order={1}>Preferences</Title>
      <AppearanceSection />
      {session.authenticated ? (
        <>
          <NotificationsSection />
          <ClaudeKeySection />
        </>
      ) : (
        <AccountSettingsLoginPrompt />
      )}
    </Stack>
  );
}
