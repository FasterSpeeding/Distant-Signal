'use client';

import { SegmentedControl, Stack, Text, useMantineColorScheme } from '@mantine/core';
import type { MantineColorScheme } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useId } from 'react';

const THEME_OPTIONS: { value: MantineColorScheme; label: string }[] = [
  { value: 'auto', label: 'Match device' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

function isColorScheme(value: string): value is MantineColorScheme {
  return value === 'auto' || value === 'light' || value === 'dark';
}

/** The colour theme picker on /account/preferences: match the device (the
 * default), light or dark. Mantine saves the choice in this browser.
 *
 * Mantine reads the saved scheme synchronously, even on the client's first
 * render, so it can disagree with the server's "auto". Showing "Match
 * device" until mounted keeps that first render identical to the server's. */
export function ThemePreference() {
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const mounted = useMounted();
  const labelId = useId();

  return (
    <Stack gap={4}>
      <Text id={labelId} size="sm" fw={500}>
        Colour theme
      </Text>
      <SegmentedControl
        aria-labelledby={labelId}
        data={THEME_OPTIONS}
        value={mounted ? colorScheme : 'auto'}
        onChange={(value) => {
          if (isColorScheme(value)) setColorScheme(value);
        }}
        maw={360}
      />
    </Stack>
  );
}
