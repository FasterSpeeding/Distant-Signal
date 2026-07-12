'use client';

import { ActionIcon, useComputedColorScheme, useMantineColorScheme } from '@mantine/core';
import type { MantineColorScheme } from '@mantine/core';

const NEXT_SCHEME: Record<MantineColorScheme, MantineColorScheme> = {
  light: 'dark',
  dark: 'auto',
  auto: 'light',
};

/** Cycles light -> dark -> auto -> light on click. The icon reflects the
 * *resolved* appearance (`useComputedColorScheme`) rather than the raw
 * preference, so picking "auto" shows whichever of sun/moon actually
 * matches the system right now instead of a generic third icon; the
 * `aria-label` states the raw preference (including "auto" itself) so
 * it's still clear which of the three states is selected. */
export function ThemeToggle() {
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const computedColorScheme = useComputedColorScheme('light');

  return (
    <ActionIcon
      variant="outline"
      onClick={() => setColorScheme(NEXT_SCHEME[colorScheme])}
      aria-label={`Theme: ${colorScheme}. Click to switch.`}
    >
      {computedColorScheme === 'dark' ? '🌙' : '☀️'}
    </ActionIcon>
  );
}
