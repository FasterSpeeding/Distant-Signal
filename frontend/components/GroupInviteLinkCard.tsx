'use client';

import { useState } from 'react';
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

export function GroupInviteLinkCard({
  groupId,
  inviteLink,
  origin,
}: {
  groupId: string;
  inviteLink: GroupInviteLink | null;
  // Resolved server-side by the caller (`app/groups/[id]/page.tsx`, via
  // `lib/siteOrigin.ts`'s `getSiteOrigin()`) rather than read here from
  // `window.location.origin` in a mount effect (review §2.11). That
  // effect-based version had `origin` read as `''` for this component's
  // first render -- this is a `'use client'` component rendered by an
  // async Server Component, so its FIRST render happens on the SERVER,
  // where `window` is undefined -- which made `url` a bare, uncopyable
  // path (`/groups/join/{token}`) and left `share()` silently inert until
  // the effect flushed. A prop resolved before this component ever
  // renders has no such window: `url` is the real absolute link on every
  // render, server and client alike, so it's usable immediately.
  origin: string;
}) {
  const router = useRouter();
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  const url = inviteLink ? `${origin}/groups/join/${inviteLink.token}` : null;

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
            {/* `aria-label` rather than a visible `label`: the "Invite
                link" heading above already names this field for a sighted
                reader, and repeating it as a Mantine `label` would put the
                same words on screen twice. Without it the field has no
                accessible name at all (axe `label`, critical) -- a screen
                reader reaching it announces only the URL's characters, with
                nothing saying what the URL is. */}
            <TextInput value={url} readOnly aria-label="Invite link" style={{ flexGrow: 1 }} />
            <Tooltip label={copied ? COPIED_LABEL : 'Share this link'}>
              {/* Mantine's default `md` ActionIcon (28px) is a hair under
                  the review's §2.10 recommendation. Sized to 36px here
                  (rather than 44px, like `ShareButton`/`PinToggle`'s
                  primary-action floor) to match the adjacent `TextInput`'s
                  own default height, since this sits flush against it in a
                  `nowrap` `Group` -- still comfortably above the 24px
                  minimum. The children are plain text glyphs, not a
                  fixed-size SVG, but `ActionIcon`'s font-size isn't tied to
                  its `--ai-size` box (`ActionIcon.css`), so they don't grow
                  with it. */}
              <ActionIcon variant="outline" color="gray" onClick={share} aria-label="Share invite link" size={36}>
                {copied ? '✓' : '⇪'}
              </ActionIcon>
            </Tooltip>
          </Group>
        ) : (
          <Text size="sm" c="dimmed">
            No active invite link.
          </Text>
        )}
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
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
