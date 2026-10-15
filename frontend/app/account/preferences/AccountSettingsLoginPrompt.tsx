import { Group } from '@mantine/core';
import { LoginControl } from '@/components/LoginControl';
import { PreferencesSection } from './PreferencesSection';

/** What a logged-out visitor sees in place of the account-backed sections
 * (notifications, the Claude API key and anything else saved to an
 * account): one short prompt to log in, rather than the sections vanishing
 * without a word. The login link returns to this page. */
export function AccountSettingsLoginPrompt() {
  return (
    <PreferencesSection
      id="account-settings-heading"
      title="Account settings"
      description="Notifications and other account settings need a Distant Signal account."
    >
      <Group>
        <LoginControl underline="always">Log in to manage notifications and more</LoginControl>
      </Group>
    </PreferencesSection>
  );
}
