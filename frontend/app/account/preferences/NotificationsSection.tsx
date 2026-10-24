import { Group } from '@mantine/core';
import { NotificationsToggle } from '@/components/NotificationsToggle';
import { TextLink } from '@/components/TextLink';
import { PreferencesSection } from './PreferencesSection';

/** Push notifications for this device, and the way to every other
 * notification setting (`/account/notifications`). Account-backed: the
 * subscription is saved to the account, so this section shows only when
 * logged in. */
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
      <TextLink href="/account/notifications" size="sm">
        Choose which notifications you get
      </TextLink>
    </PreferencesSection>
  );
}
