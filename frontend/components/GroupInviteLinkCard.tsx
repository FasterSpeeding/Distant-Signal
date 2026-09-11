'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Button, Card, Group, Stack, Text, TextInput, Tooltip } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
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
  const needsLoginState = useNeedsLogin();

  // `window` is read in an effect, never in the render body: this is a
  // `'use client'` component rendered by an async Server Component
  // (`app/groups/[id]/page.tsx`), so its FIRST render happens on the
  // SERVER, where `window` is undefined -- touching it during render threw
  // a `ReferenceError` for every admin/owner of every group, including
  // straight after `CreateGroupForm` mints the first link and navigates
  // here. `origin` is `''` for that first render, so `url` reads as the
  // relative `/groups/join/{token}` for exactly one render and
  // self-corrects on the effect flush; `share()` is gated on `origin`
  // separately below so a relative link can never be copied or shared.
  const [origin, setOrigin] = useState('');
  useEffect(() => setOrigin(window.location.origin), []);

  const url = inviteLink ? `${origin}/groups/join/${inviteLink.token}` : null;

  async function share() {
    // `origin === ''` only before the mount effect has run, i.e. never by
    // the time a real user can click -- the guard just makes it impossible
    // to share the one-render relative form of `url`.
    if (!url || origin === '') return;
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
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/invite-link`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError('Could not create a new invite link.');
        }
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
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/invite-link`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError('Could not revoke the invite link.');
        }
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
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to manage this invite link</LoginLink>
        )}
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
