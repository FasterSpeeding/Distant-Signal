'use client';

import { ActionIcon, useComputedColorScheme, useMantineColorScheme } from '@mantine/core';
import type { MantineColorScheme } from '@mantine/core';
import { useMounted } from '@mantine/hooks';

const NEXT_SCHEME: Record<MantineColorScheme, MantineColorScheme> = {
  light: 'dark',
  dark: 'auto',
  auto: 'light',
};

/** Icon for light theme. */
function IconSun() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <line x1="21" y1="12" x2="23" y2="12" />
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
    </svg>
  );
}

/** Icon for dark theme. */
function IconMoon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

/** Icon for auto theme. */
function IconSunMoon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

/** Cycles light -> dark -> auto -> light on click. The icon reflects the
 * *resolved* appearance (`useComputedColorScheme`) rather than the raw
 * preference, so picking "auto" shows the `IconSunMoon` instead of a
 * sun/moon alone; the `aria-label` states the raw preference (including
 * "auto" itself) so it's still clear which of the three states is selected.
 *
 * The three distinct icons (`IconSun` / `IconMoon` / `IconSunMoon`) are
 * visually unambiguous without needing an additional badge overlay: clicking
 * from "auto" (resolved to light, showing sun) to explicit "light" changes
 * the icon from the composite sun-moon to the sun alone, making the click
 * visible.
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
  const mounted = useMounted();

  const displayedScheme = mounted ? colorScheme : 'auto';
  const displayedComputedScheme = mounted ? computedColorScheme : 'light';

  let icon: React.ReactNode;
  if (displayedScheme === 'auto') {
    icon = <IconSunMoon />;
  } else if (displayedComputedScheme === 'dark') {
    icon = <IconMoon />;
  } else {
    icon = <IconSun />;
  }

  return (
    <ActionIcon
      variant="outline"
      onClick={() => setColorScheme(NEXT_SCHEME[colorScheme])}
      aria-label={`Theme: ${displayedScheme}. Click to switch.`}
    >
      {icon}
    </ActionIcon>
  );
}
