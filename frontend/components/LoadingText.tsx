import type { ReactNode } from 'react';
import { Group, Loader, Text, type MantineSize } from '@mantine/core';

/** In-progress copy: a plain word or phrase ("Searching", "Loading your
 * journeys") beside a small spinner. The spinner, not a trailing "…",
 * says the work is still going (docs/style-guide.md, Writing). It is
 * `aria-hidden`; the words are what a screen reader reads. */
export function LoadingText({
  children,
  size = 'sm',
  role,
  'aria-live': ariaLive,
}: {
  children: ReactNode;
  size?: MantineSize;
  role?: 'status';
  'aria-live'?: 'polite';
}) {
  return (
    <Group gap="xs" wrap="nowrap" data-loading-text>
      <Loader size="xs" aria-hidden="true" />
      <Text size={size} c="dimmed" role={role} aria-live={ariaLive}>
        {children}
      </Text>
    </Group>
  );
}

/** A busy button's `leftSection`, beside a label such as "Saving". */
export function ButtonSpinner() {
  return <Loader size={14} color="currentColor" aria-hidden="true" />;
}
