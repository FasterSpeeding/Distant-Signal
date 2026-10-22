import { Group } from '@mantine/core';
import { JourneySuggestionsToggle } from '@/components/JourneySuggestionsToggle';
import { NotificationsToggle } from '@/components/NotificationsToggle';
import { PreferencesSection } from './PreferencesSection';

/** Push notifications for this device. Account-backed: the subscription
 * is saved to the account, so this section shows only when logged in. */
export function NotificationsSection() {
  return (
    <PreferencesSection
      id="notifications-heading"
      title="Notifications"
      description="Alerts on this device when a line you've pinned or a train you track is disrupted."
    >
      <Group>
        <NotificationsToggle />
      </Group>
      <JourneySuggestionsToggle />
    </PreferencesSection>
  );
}
