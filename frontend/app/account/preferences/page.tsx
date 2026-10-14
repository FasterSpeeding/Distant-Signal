import { Card, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { ClaudeKeySettings } from '@/components/ClaudeKeySettings';
import { LoginControl } from '@/components/LoginControl';
import { NotificationsToggle } from '@/components/NotificationsToggle';
import { PrideModeSelect } from '@/components/PrideMode';
import { SectionTitle } from '@/components/SectionTitle';
import { ThemePreference } from '@/components/ThemePreference';
import { getChatbotAccess, getSessionOrLoggedOut } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';

// Session-dependent: never prerender (see app/page.tsx's own comment).
export const revalidate = 0;

export const metadata: Metadata = {
  ...pageMetadata('Preferences', 'Choose a colour theme, turn on notifications and set other options.'),
  robots: { index: false },
};

/** `/account/preferences` (round 2, item 4): the one place for settings
 * that used to be scattered. The colour theme was a nav button, pride mode
 * had no control at all after round 1, "Enable notifications" was a large
 * button on the home page even for logged-out visitors, and the Claude API
 * key sat at the top of /chat.
 *
 * Open to everyone: the theme and pride flag are saved in the browser and
 * need no account. Notifications need one, so a logged-out visitor gets a
 * log in link there instead of a button that would ask for browser
 * permission first and only then fail. The Claude API key section shows
 * only to accounts that can use "Ask about trains". */
export default async function PreferencesPage() {
  const [session, chatAccess] = await Promise.all([getSessionOrLoggedOut(), getChatbotAccess()]);

  return (
    <Stack p="lg" gap="lg" maw={640}>
      <Title order={1}>Preferences</Title>

      <Card withBorder component="section" aria-labelledby="appearance-heading">
        <Stack gap="md">
          <Stack gap={4}>
            <SectionTitle id="appearance-heading">Appearance</SectionTitle>
            <Text size="sm" c="dimmed">
              Saved in this browser.
            </Text>
          </Stack>
          <ThemePreference />
          <PrideModeSelect />
        </Stack>
      </Card>

      <Card withBorder component="section" aria-labelledby="notifications-heading">
        <Stack gap="sm" align="flex-start">
          <SectionTitle id="notifications-heading">Notifications</SectionTitle>
          <Text size="sm">Alerts on this device when a line you&apos;ve pinned or a train you track is disrupted.</Text>
          {session.authenticated ? (
            <NotificationsToggle />
          ) : (
            <LoginControl underline="always">Log in to turn on notifications</LoginControl>
          )}
        </Stack>
      </Card>

      {chatAccess.status === 'allowed' && (
        <Card withBorder component="section" aria-labelledby="claude-key-heading">
          <Stack gap="sm">
            <SectionTitle id="claude-key-heading">Ask about trains</SectionTitle>
            <Text size="sm">Uses Claude with your own API key.</Text>
            <ClaudeKeySettings />
          </Stack>
        </Card>
      )}
    </Stack>
  );
}
