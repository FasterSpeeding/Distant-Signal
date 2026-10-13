'use client';

import { useState } from 'react';
import { List, Text, TextInput } from '@mantine/core';
import { ConfirmActionButton } from './ConfirmActionButton';
import { clearBrowserAccountData } from '@/lib/browserAccountData';

/** The phrase the visitor types to confirm, and the exact value `DELETE
 * /public/account` requires in its `confirm` body field
 * (`crates/api/src/data/account.rs`'s `DELETE_ACCOUNT_CONFIRMATION`). The
 * backend compares case-insensitively; so does this. */
export const DELETE_ACCOUNT_CONFIRMATION = 'delete my account';

/** "Delete my account" (UK legal audit LEG-4; UK GDPR Art. 17). Opens a
 * confirmation step that spells out what is deleted, what happens to
 * groups and backups, and what is held elsewhere, then requires the
 * visitor to type {@link DELETE_ACCOUNT_CONFIRMATION} before the delete
 * button is enabled. On success the session cookie is already cleared by
 * the response; this clears browser-only data and moves to
 * `/account/deleted`. */
export function DeleteAccountButton() {
  const [typed, setTyped] = useState('');
  const confirmed = typed.trim().toLowerCase() === DELETE_ACCOUNT_CONFIRMATION;

  return (
    <ConfirmActionButton
      label="Delete my account"
      confirmLabel="Delete my account permanently"
      actionName="delete my account permanently"
      title="Delete your account?"
      request={() =>
        fetch('/api/account', {
          method: 'DELETE',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ confirm: DELETE_ACCOUNT_CONFIRMATION }),
        })
      }
      failure={['delete', 'your account']}
      unauthorizedMessage="Your session has expired. Log in again, then delete your account."
      networkFailureNote="Nothing was deleted."
      confirmDisabled={!confirmed}
      onCancel={() => setTyped('')}
      onSuccess={(router) => {
        clearBrowserAccountData();
        router.push('/account/deleted');
        router.refresh();
      }}
    >
      {(busy) => (
        <>
          <Text>This permanently deletes your Distant Signal account and everything in it:</Text>
          <List size="sm" spacing={4}>
            <List.Item>tracked trains, tickets, journeys and journey templates</List.Item>
            <List.Item>pinned lines, stations and operators, and your custom lines</List.Item>
            <List.Item>push notification subscriptions and all your sessions, on every device</List.Item>
            <List.Item>share links and invite links you created</List.Item>
            <List.Item>
              your group memberships, and the trains, journeys and custom lines you shared into groups
            </List.Item>
          </List>
          <Text size="sm">
            Groups carry on for their other members. If you own a group, it passes to its longest-standing admin (or
            member); a group with no other members is deleted.
          </Text>
          <Text size="sm">Our database backups are encrypted, and your data can stay in them for up to 14 days.</Text>
          <Text size="sm">
            You log in through a separate single sign-on account (or Discord), which this does not delete. Close that
            account there if you want it gone too.
          </Text>
          <Text size="sm">You may want to download your data first. This can&apos;t be undone.</Text>
          <TextInput
            label={`Type "${DELETE_ACCOUNT_CONFIRMATION}" to confirm`}
            value={typed}
            onChange={(event) => setTyped(event.currentTarget.value)}
            autoComplete="off"
            disabled={busy}
          />
        </>
      )}
    </ConfirmActionButton>
  );
}
