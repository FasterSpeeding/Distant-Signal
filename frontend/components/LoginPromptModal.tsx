'use client';

import { Suspense } from 'react';
import Link from 'next/link';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useLoginHref } from './useLoginHref';

/** Isolates the one `useLoginHref()` call (and therefore
 * `usePathname()`/`useSearchParams()`) behind its own `<Suspense>`
 * boundary below -- `next build`'s static prerendering of a route with no
 * dynamic segment (e.g. `/lines/new`, which mounts `CustomLineForm`
 * unconditionally) fails with "useSearchParams() should be wrapped in a
 * suspense boundary" otherwise, since `LoginPromptModal` itself is now
 * always mounted (not just on a 401 the way the old inline `LoginLink`
 * was). A real gap the modal-login-prompt plan/design didn't anticipate,
 * found by actually running `npm run build`, not just `vitest`/`tsc`.
 * Plain `<Link>` wrapping `Button`, not `component={Link}` on the Mantine
 * polymorphic prop -- established convention regardless of Server/Client
 * boundary, see `CustomLineForm.tsx:236-240`'s own Cancel button and this
 * design's Decision 3.
 *
 * `prefetch={false}`: same reasoning as `LoginLink.tsx`'s own doc comment
 * -- this href is `crates/api/src/routes/auth.rs`'s `login` handler, not a
 * real page, and Next's default prefetch-on-visibility would otherwise
 * fire that handler's real side effects (a new `login_state` DB row, a
 * fresh `login_state` cookie) the moment this link scrolls into view.
 * Modal's own `keepMounted: false` default means this link isn't in the
 * DOM at all while `opened` is `false`, so the exposure here is narrower
 * than the always-mounted nav-bar `LoginLink` -- but once a visitor
 * actually opens one of these prompts, the same unwanted-background-fetch
 * risk applies for as long as it stays open. */
function LoginButtonLink() {
  const href = useLoginHref();
  return (
    <Link href={href} style={{ textDecoration: 'none' }} prefetch={false}>
      <Button>Log in</Button>
    </Link>
  );
}

/** Thin, fully-controlled presentational wrapper -- mirrors
 * `DeleteLineButton.tsx:66-83`/`DeleteTrainButton.tsx:76-93`'s own
 * `<Modal opened={opened} onClose={close}>` usage. Every migrated call
 * site renders this unconditionally; Mantine's `Modal` already no-ops
 * visually when `opened` is `false`, so callers don't need their own
 * `{needsLogin && ...}` guard the way the inline `LoginLink` version
 * required. `children` is call-site-specific body prose (mirrors
 * `LoginLink`'s existing flexibility) -- there is deliberately no `verb`
 * prop; the title is a fixed constant, never a prop. See
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
 * Decisions 1-3.
 *
 * `closeButtonProps={{ 'aria-label': 'Close' }}`: Mantine's `Modal` close
 * button has no default accessible name (confirmed by reading the
 * installed `@mantine/core` source -- neither `ModalBaseCloseButton` nor
 * the underlying `CloseButton` sets one), so this is set explicitly for
 * basic accessibility, not just test convenience. */
export function LoginPromptModal({
  opened,
  onClose,
  children,
}: {
  opened: boolean;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <Modal opened={opened} onClose={onClose} title="Log in required" closeButtonProps={{ 'aria-label': 'Close' }}>
      <Text>{children}</Text>
      <Group justify="end" mt="md">
        <Suspense fallback={<Button disabled>Log in</Button>}>
          <LoginButtonLink />
        </Suspense>
      </Group>
    </Modal>
  );
}
