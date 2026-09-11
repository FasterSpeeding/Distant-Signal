'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Button, Card, Group, Stack, Text, TextInput, Tooltip } from '@mantine/core';
import type { GroupInviteLink } from '@/lib/types';

/** Copy-to-clipboard / Web Share affordance for a group's invite link,
 * adapted from `ShareButton.tsx`'s own pattern (feature-detect
 * `navigator.share`, fall back to the clipboard, flip to "Copied!" for a
 * couple of seconds) -- parameterized by an explicit `url` rather than
 * `window.location.href`, since this shares a DIFFERENT page (the join
 * page) than the one it's rendered on. `admin`/`owner`-only: the caller
 * (Task 12's page) never renders this for a plain `member` at all,
 * matching `inviteLink` being `null` in that case on the wire already. */
const COPIED_LABEL = 'Copied!';
const COPIED_TIMEOUT_MS = 2000;

export function GroupInviteLinkCard({ groupId, inviteLink }: { groupId: string; inviteLink: GroupInviteLink | null }) {
  const router = useRouter();
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const url = inviteLink ? `${window.location.origin}/groups/join/${inviteLink.token}` : null;

  async function share() {
    if (!url) return;
    if (typeof navigator.share === 'function') {
      try {
        await navigator.share({ url, title: 'Join my group on Distant Signal' });
        return;
      } catch (err) {
        if (err && typeof err === 'object' && 'name' in err && err.name === 'AbortError') return;
      }
    }
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), COPIED_TIMEOUT_MS);
    } catch {
      // No more to do -- see ShareButton.tsx's own identical fallback.
    }
  }

  async function regenerate() {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/groups/${groupId}/invite-link`, { method: 'POST' });
      if (!response.ok) {
        setError('Could not create a new invite link.');
        setBusy(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Could not create a new invite link.');
      setBusy(false);
    }
  }

  async function revoke() {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/groups/${groupId}/invite-link`, { method: 'DELETE' });
      if (!response.ok) {
        setError('Could not revoke the invite link.');
        setBusy(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Could not revoke the invite link.');
      setBusy(false);
    }
  }

  return (
    <Card withBorder>
      <Stack gap="xs">
        <Text fw={500}>Invite link</Text>
        {url ? (
          <Group gap="xs" wrap="nowrap">
            <TextInput value={url} readOnly style={{ flexGrow: 1 }} />
            <Tooltip label={copied ? COPIED_LABEL : 'Share this link'}>
              <ActionIcon variant="outline" color="gray" onClick={share} aria-label="Share invite link">
                {copied ? '✓' : '⇪'}
              </ActionIcon>
            </Tooltip>
          </Group>
        ) : (
          <Text size="sm" c="dimmed">
            No active invite link.
          </Text>
        )}
        {error && <Text c="red">{error}</Text>}
        <Group gap="xs">
          <Button variant="default" size="xs" onClick={regenerate} loading={busy}>
            Regenerate
          </Button>
          {url && (
            <Button variant="outline" color="red" size="xs" onClick={revoke} loading={busy}>
              Revoke
            </Button>
          )}
        </Group>
      </Stack>
    </Card>
  );
}
