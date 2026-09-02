'use client';

import Link from 'next/link';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useLoginHref } from './useLoginHref';

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
  const href = useLoginHref();
  return (
    <Modal opened={opened} onClose={onClose} title="Log in required" closeButtonProps={{ 'aria-label': 'Close' }}>
      <Text>{children}</Text>
      <Group justify="end" mt="md">
        {/* Plain `<Link>` wrapping `Button`, not `component={Link}` on the
            Mantine polymorphic prop -- established convention regardless of
            Server/Client boundary, see `CustomLineForm.tsx:236-240`'s own
            Cancel button and this design's Decision 3. */}
        <Link href={href} style={{ textDecoration: 'none' }}>
          <Button>Log in</Button>
        </Link>
      </Group>
    </Modal>
  );
}
