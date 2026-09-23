'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Button, Group, Modal, Stack, Text, TextInput, Tooltip } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { JourneyShareLink } from '@/lib/types';

const COPIED_LABEL = 'Copied!';
const COPIED_TIMEOUT_MS = 2000;

/** The journey-detail-page "unlisted link" control -- a deliberate hybrid
 * of two existing precedents, per the plan's Task 4 brief
 * (docs/superpowers/specs/2026-09-23-unlisted-links-design.md): the
 * Button-that-opens-a-Modal SHAPE is `ShareJourneyButton.tsx`'s own (this
 * page's action row is buttons, not cards, so there's nowhere for an
 * always-visible `GroupInviteLinkCard`-style card to live), but the modal
 * BODY -- URL field, copy/share icon, Regenerate/Revoke -- is
 * `GroupInviteLinkCard.tsx`'s content, including its exact fetch/401/
 * `router.refresh()` pattern.
 *
 * Two button labels rather than one, unlike either precedent (`
 * ShareJourneyButton` always reads "Share with a group";
 * `GroupInviteLinkCard` is always-visible, never behind a trigger button
 * at all): "Get shareable link" when `shareLink` is `null` and "Manage
 * shared link" once one exists, so a caller can tell, without opening the
 * modal, whether a click will offer to CREATE the first link or manage an
 * EXISTING one -- the one piece of copy this component invents, per the
 * brief's own instruction to justify it.
 *
 * No expiry line in the modal, unlike `GroupInviteLinkCard`'s "Expires
 * {date}." -- design spec §5's own choice is no TTL for a journey's
 * unlisted link (`expiresAt` is always `null` on the wire today), so
 * there is nothing dated to report. In its place, one static line stating
 * the plain-language equivalent of that same §5 reasoning ("no automatic
 * TTL, explicit revoke is the lever") for the audience that actually needs
 * it: the owner deciding whether it's still safe to have this link out
 * there.
 *
 * `shareLink`/`origin` are both server-resolved props, passed down from
 * `app/journeys/[id]/page.tsx` exactly the way `GroupInviteLinkCard`
 * receives `inviteLink`/`origin` from `app/groups/[id]/page.tsx` -- see
 * that component's own doc comment for why `origin` must be a prop
 * resolved before this ever renders (`lib/siteOrigin.ts::getSiteOrigin()`)
 * rather than read from `window.location.origin` in a mount effect: this
 * is a `'use client'` component whose first render happens on the SERVER
 * (rendered by an async Server Component), where a link pointing at a
 * DIFFERENT page than the one it's rendered on can't fall back to a
 * relative path. */
export function ShareJourneyLinkButton({
  journeyId,
  shareLink,
  origin,
}: {
  journeyId: number;
  shareLink: JourneyShareLink | null;
  origin: string;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  const url = shareLink ? `${origin}/journeys/shared/${shareLink.token}` : null;

  function handleOpen() {
    setError(null);
    setCopied(false);
    needsLoginState.reset();
    open();
  }

  async function share() {
    if (!url) return;
    if (typeof navigator.share === 'function') {
      try {
        await navigator.share({ url, title: 'View my journey on Distant Signal' });
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
      // No more to do -- see GroupInviteLinkCard's own identical fallback.
    }
  }

  async function createOrRegenerate() {
    setBusy(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/share-link`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError(shareLink ? 'Could not create a new share link.' : 'Could not create a share link.');
        }
        setBusy(false);
        return;
      }
      router.refresh();
    } catch {
      setError(shareLink ? 'Could not create a new share link.' : 'Could not create a share link.');
      setBusy(false);
    }
  }

  async function revoke() {
    setBusy(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/share-link`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError('Could not revoke the share link.');
        }
        setBusy(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Could not revoke the share link.');
      setBusy(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        {shareLink ? 'Manage shared link' : 'Get shareable link'}
      </Button>
      <Modal opened={opened} onClose={close} title="Share this journey">
        <Stack gap="xs">
          {url ? (
            <Group gap="xs" wrap="nowrap">
              {/* `aria-label`, not a visible `label`: the modal's own title
                  already names this field for a sighted reader -- see
                  `GroupInviteLinkCard`'s identical field for the same
                  reasoning. */}
              <TextInput value={url} readOnly aria-label="Share link" style={{ flexGrow: 1 }} />
              <Tooltip label={copied ? COPIED_LABEL : 'Share this link'}>
                <ActionIcon variant="outline" color="gray" onClick={share} aria-label="Share link" size={36}>
                  {copied ? '✓' : '⇪'}
                </ActionIcon>
              </Tooltip>
            </Group>
          ) : (
            <Text size="sm" c="dimmed">
              No active share link.
            </Text>
          )}
          {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
          {needsLoginState.needsLogin && (
            <LoginLink underline="always">Log in to manage this journey&apos;s share link</LoginLink>
          )}
          <Group gap="xs">
            <Button variant="default" size="xs" onClick={createOrRegenerate} loading={busy}>
              {shareLink ? 'Regenerate' : 'Create link'}
            </Button>
            {shareLink && (
              <Button variant="outline" color="red" size="xs" onClick={revoke} loading={busy}>
                Revoke
              </Button>
            )}
          </Group>
          {/* The plain-language equivalent of design spec §5's own
              reasoning ("no automatic TTL, explicit revoke is the lever")
              for the one audience that actually needs to know it: the
              owner deciding whether it's still fine for this link to be
              out there. Static and unconditional (unlike
              `GroupInviteLinkCard`'s "Regenerating invalidates..." line,
              which is gated on an existing link) since it's true whether or
              not a link exists yet -- it's describing what creating one
              will mean. */}
          <Text size="xs" c="dimmed">
            Anyone with this link can view this journey (read-only) without logging in, until you revoke it.
          </Text>
        </Stack>
      </Modal>
    </>
  );
}
