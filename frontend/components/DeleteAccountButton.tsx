'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, List, Modal, Stack, Text, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';

/** The phrase the visitor types to confirm, and the exact value `DELETE
 * /public/account` requires in its `confirm` body field
 * (`crates/api/src/data/account.rs`'s `DELETE_ACCOUNT_CONFIRMATION`). The
 * backend compares case-insensitively; so does this. */
export const DELETE_ACCOUNT_CONFIRMATION = 'delete my account';

/** Browser-only data this app keeps for a signed-in visitor: the chat's
 * MCP OAuth client and tokens (`lib/mcpOAuthProvider.ts`, keys prefixed
 * `ds-mcp-oauth:`, listed in `BROWSER_ACCOUNT_KEYS` below) and the visitor's own Anthropic key
 * (`lib/anthropicKey.ts`). Neither is ever sent to Distant Signal, so the
 * backend cannot delete them; deleting the account clears them here. */
function clearBrowserAccountData() {
  try {
    for (const key of BROWSER_ACCOUNT_KEYS) {
      localStorage.removeItem(key);
    }
  } catch {
    // Storage blocked (private mode, disabled site data): nothing stored.
  }
}

const BROWSER_ACCOUNT_KEYS = [
  'ds-mcp-oauth:client-information',
  'ds-mcp-oauth:tokens',
  'ds-mcp-oauth:code-verifier',
  'ds-mcp-oauth:oauth-state',
  'ds-anthropic-api-key',
];

/** "Delete my account" (UK legal audit LEG-4; UK GDPR Art. 17). Opens a
 * confirmation step that spells out what is deleted, what happens to
 * groups and backups, and what is held elsewhere, then requires the
 * visitor to type {@link DELETE_ACCOUNT_CONFIRMATION} before the delete
 * button is enabled. On success the session cookie is already cleared by
 * the response; this clears browser-only data and moves to
 * `/account/deleted`. */
export function DeleteAccountButton() {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [typed, setTyped] = useState('');
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirmed = typed.trim().toLowerCase() === DELETE_ACCOUNT_CONFIRMATION;

  function handleClose() {
    if (deleting) return;
    setTyped('');
    setError(null);
    close();
  }

  async function handleDelete() {
    if (!confirmed) return;
    setDeleting(true);
    setError(null);
    try {
      const response = await fetch('/api/account', {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ confirm: DELETE_ACCOUNT_CONFIRMATION }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          setError('Your session has expired. Log in again, then delete your account.');
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      clearBrowserAccountData();
      router.push('/account/deleted');
      router.refresh();
    } catch {
      setError('Request failed. Nothing was deleted; try again.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button color="red" variant="outline" onClick={open}>
        Delete my account
      </Button>
      <Modal opened={opened} onClose={handleClose} title="Delete your account?">
        <Stack gap="sm">
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
          <Text size="sm">
            Our database backups are encrypted and kept for 7 days, so your data is gone from them within 7 days
            too.
          </Text>
          <Text size="sm">
            You sign in through a separate single sign-on account (or Discord), which this does not delete. Close
            that account there if you want it gone too.
          </Text>
          <Text size="sm">
            You may want to download your data first. This can&apos;t be undone.
          </Text>
          <TextInput
            label={`Type "${DELETE_ACCOUNT_CONFIRMATION}" to confirm`}
            value={typed}
            onChange={(event) => setTyped(event.currentTarget.value)}
            autoComplete="off"
            disabled={deleting}
          />
          {error && (
            <Text c="var(--ds-color-error-text)" role="alert">
              {error}
            </Text>
          )}
          <Group justify="end" mt="xs">
            <Button variant="default" onClick={handleClose} disabled={deleting}>
              Cancel
            </Button>
            <Button color="red" onClick={handleDelete} loading={deleting} disabled={!confirmed}>
              Delete my account permanently
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
}
