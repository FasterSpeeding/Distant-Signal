import type { ReactElement } from 'react';
import { render, type RenderOptions } from '@testing-library/react';
import { MantineProvider, type MantineColorScheme } from '@mantine/core';
import { theme } from '@/lib/theme';

type RenderWithMantineOptions = RenderOptions & {
  // Only ThemeToggle's tests need this (it asserts behaviour starting from
  // the "auto" scheme); every other caller relies on Mantine's own default
  // ('light') by simply not passing it.
  defaultColorScheme?: MantineColorScheme;
};

// The single place a test wraps its subject in `MantineProvider`. Every
// caller gets the real production theme (`lib/theme.ts`, the same object
// `app/layout.tsx` passes) for free — a test file can no longer render
// under a different, hand-rolled provider and still pass. See
// `lib/theme.test.tsx` for the regression check that exercises this.
export function renderWithMantine(ui: ReactElement, options: RenderWithMantineOptions = {}) {
  const { defaultColorScheme, ...renderOptions } = options;
  return render(
    <MantineProvider theme={theme} defaultColorScheme={defaultColorScheme}>
      {ui}
    </MantineProvider>,
    renderOptions,
  );
}
