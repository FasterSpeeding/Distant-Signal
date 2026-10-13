'use client';

import { Group, Modal, Text } from '@mantine/core';
import { LoginConsentNote } from './LoginConsentNote';
import { LoginControl } from './LoginControl';

/** Thin, fully-controlled presentational wrapper -- mirrors
 * `ConfirmActionButton`'s own
 * `<Modal opened={opened} onClose={close}>` usage. The button is
 * `LoginControl`'s button variant, which owns the `<Suspense>` boundary a
 * statically prerendered page (`/lines/new`) needs around `useLoginHref`. Every migrated call
 * site renders this unconditionally; Mantine's `Modal` already no-ops
 * visually when `opened` is `false`, so callers don't need their own
 * `{needsLogin && ...}` guard the way the inline login link version
 * required. `children` is call-site-specific body prose (mirrors
 * `LoginControl`'s flexibility) -- there is deliberately no `verb`
 * prop; the title is a fixed constant, never a prop. See
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
 * Decisions 1-3.
 *
 * `closeButtonProps={{ 'aria-label': 'Close' }}`: Mantine's `Modal` close
 * button has no default accessible name (confirmed by reading the
 * installed `@mantine/core` source -- neither `ModalBaseCloseButton` nor
 * the underlying `CloseButton` sets one), so this is set explicitly for
 * basic accessibility, not just test convenience.
 *
 * That gap is now ALSO closed app-wide, in `lib/theme.ts`'s
 * `components.Modal.defaultProps` -- the other fifteen modals in this app
 * had the same defect and an axe sweep of their opened states found all of
 * them. This line is therefore no longer load-bearing on its own, and is
 * kept only because it is the one place the Mantine-source reading above
 * is written down; `useProps` merges call-site props OVER theme defaults,
 * so the two agreeing is harmless. Delete this prop only together with
 * that paragraph, and read `lib/theme.ts`'s shallow-merge caveat first. */
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
      {/* LEG-1: only once the terms and privacy notice are published. */}
      <LoginConsentNote mt="sm" />
      <Group justify="end" mt="md">
        {/* The consent note sits above, beside the body text. */}
        <LoginControl variant="button" consentNote={false}>
          Log in
        </LoginControl>
      </Group>
    </Modal>
  );
}
