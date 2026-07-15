'use client';

import { useEffect, useState } from 'react';
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
 * it's still clear which of the three states is selected.
 *
 * Mantine's `colorScheme` reads localStorage synchronously (even on the
 * client's first, pre-hydration render), so it can already disagree with
 * the server-rendered "auto" default before React ever gets to diff the
 * tree. Rendering the layout's default until after mount keeps that first
 * client render identical to the server output; the real, possibly-stored
 * preference then takes over post-hydration. */
export function ThemeToggle() {
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const computedColorScheme = useComputedColorScheme('light');
  const [mounted, setMounted] = useState(false);

  useEffect(() => setMounted(true), []);

  const displayedScheme = mounted ? colorScheme : 'auto';
  const displayedComputedScheme = mounted ? computedColorScheme : 'light';

  return (
    <ActionIcon
      variant="outline"
      onClick={() => setColorScheme(NEXT_SCHEME[colorScheme])}
      aria-label={`Theme: ${displayedScheme}. Click to switch.`}
    >
      {displayedComputedScheme === 'dark' ? '🌙' : '☀️'}
    </ActionIcon>
  );
}
