'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Group, Stack, Text } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginControl } from './LoginControl';
import { CopyableLink } from './CopyableLink';
import { formatDate } from '@/lib/dateFormat';
import type { GroupInviteLink } from '@/lib/types';
import { freshTokenFromResponse } from '@/lib/freshLinkToken';
import { describeFailure } from '@/lib/failure';

/** Copy-to-clipboard / Web Share affordance for a group's invite link,
 * adapted from `ShareButton.tsx`'s own pattern (feature-detect
 * `navigator.share`, fall back to the clipboard, flip to "Copied!" for a
 * couple of seconds) -- parameterized by an explicit `url` rather than
 * `window.location.href`, since this shares a DIFFERENT page (the join
 * page) than the one it's rendered on. `admin`/`owner`-only: the caller
 * (Task 12's page) never renders this for a plain `member` at all,
 * matching `inviteLink` being `null` in that case on the wire already. */

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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  // Tokens are stored hashed server-side (L14), so the server-rendered
  // `inviteLink` never carries one -- the only time this component can show
  // a copyable URL is right after THIS session's own Regenerate, from the
  // POST response. Otherwise an active link is known only by its expiry.
  const [freshToken, setFreshToken] = useState<string | null>(null);
  const token = freshToken ?? inviteLink?.token ?? null;
  const url = token ? `${origin}/groups/join/${token}` : null;
  const hasActiveLink = inviteLink !== null || freshToken !== null;

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
          setError(describeFailure('create', 'a new invite link', response.status));
        }
        setBusy(false);
        return;
      }
      setFreshToken(await freshTokenFromResponse(response));
      router.refresh();
      setBusy(false);
    } catch {
      setError(describeFailure('create', 'a new invite link'));
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
          setError(describeFailure('revoke', 'the invite link', response.status));
        }
        setBusy(false);
        return;
      }
      setFreshToken(null);
      router.refresh();
      setBusy(false);
    } catch {
      setError(describeFailure('revoke', 'the invite link'));
      setBusy(false);
    }
  }

  return (
    <Card withBorder>
      <Stack gap="xs">
        <Text fw={500}>Invite link</Text>
        {url ? (
          <CopyableLink value={url} label="Invite link" hideLabel share shareTitle="Join my group on Distant Signal" />
        ) : hasActiveLink ? (
          <Text size="sm" c="dimmed">
            An invite link is active, but for security it can&apos;t be shown again. Regenerate to get a new link to
            copy.
          </Text>
        ) : (
          <Text size="sm" c="dimmed">
            No active invite link.
          </Text>
        )}
        {/* Review §3.2.2: nothing here told a viewer this link ever
            expires (spec §2.3 gives every link a 7-day life) -- an owner
            who bookmarked or pinned an old link would only discover it had
            gone stale when a new joiner's click 404'd. `inviteLink` (not a
            derived `url`) is the source, so this renders even though
            `url` already folds the token into an absolute path above. */}
        {inviteLink && (
          <Text size="xs" c="dimmed">
            Expires {formatDate(inviteLink.expiresAt)}.
          </Text>
        )}
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginControl underline="always">Log in to manage this invite link</LoginControl>
        )}
        <Group gap="xs">
          <Button variant="default" size="xs" onClick={regenerate} loading={busy}>
            Regenerate
          </Button>
          {hasActiveLink && (
            <Button variant="outline" color="red" size="xs" onClick={revoke} loading={busy}>
              Revoke
            </Button>
          )}
        </Group>
        {/* Same review note as the expiry line above: "Regenerate" doesn't
            just extend the current link's life, it swaps the token
            entirely (`rotate_invite_link`, `crates/api/src/data/groups.rs`)
            -- anyone still holding the old URL gets a 404 the moment this
            fires. Placed under the buttons rather than beside them so it
            reads as an explanation of what the row above just did, not as
            a third button. Gated on `url` -- with no active link there is
            no "old one" for Regenerate to invalidate; it's just creating
            the first one. */}
        {hasActiveLink && (
          <Text size="xs" c="dimmed">
            Regenerating creates a new link. The old one stops working immediately.
          </Text>
        )}
      </Stack>
    </Card>
  );
}
