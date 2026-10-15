'use client';

import { useEffect, useState } from 'react';
import { Alert, Text } from '@mantine/core';
import { TextLink } from './TextLink';
import { formatDateTime } from '@/lib/dateFormat';
import type { ConnectedAppGrant } from '@/lib/types';

const STORAGE_PREFIX = 'connected-app-notice-dismissed:';

function storageKey(notice: ConnectedAppGrant): string {
  return `${STORAGE_PREFIX}${notice.client}:${notice.createdAt}`;
}

/** The banner every page shows for 7 days after an app is connected
 * (delegated-access spec §5.3, compensating control 2): which app, when,
 * and where to disconnect it if it wasn't the visitor. Dismissing it hides
 * it in this browser for that connection; a new connection shows it again.
 * The api only returns a notice for a grant made in the last 7 days. */
export function ConnectedAppNotice({ notice }: { notice: ConnectedAppGrant }) {
  // Hidden until mounted, so a dismissed notice never flashes in.
  const [dismissed, setDismissed] = useState(true);
  useEffect(() => {
    try {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- reads localStorage only after mount, so the server and first client render agree
      setDismissed(window.localStorage.getItem(storageKey(notice)) === '1');
    } catch {
      setDismissed(false);
    }
  }, [notice]);

  if (dismissed) return null;
  return (
    <Alert
      color="blue"
      title={`${notice.displayName} was connected to your account`}
      withCloseButton
      closeButtonLabel="Dismiss"
      onClose={() => {
        try {
          window.localStorage.setItem(storageKey(notice), '1');
        } catch {
          // Private mode: hidden for this page only.
        }
        setDismissed(true);
      }}
      mb="md"
    >
      <Text size="sm">
        Connected {formatDateTime(notice.createdAt)}. Not you?{' '}
        <TextLink href="/account/connected-apps" inline underline="always">
          Disconnect it in Connected apps
        </TextLink>
        .
      </Text>
    </Alert>
  );
}
