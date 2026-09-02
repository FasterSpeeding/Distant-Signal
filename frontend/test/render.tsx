import type { ReactNode } from 'react';
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
//
// `ui` is typed as `ReactNode` (not `ReactElement`) because several server
// components under test (e.g. TicketPanel's 404/not-the-owner branch)
// legitimately `return null`, and `@testing-library/react`'s own `render`
// already accepts `ReactNode` -- narrowing to `ReactElement` here only
// rejected a value the underlying `render` call handles fine.
export function renderWithMantine(ui: ReactNode, options: RenderWithMantineOptions = {}) {
  const { defaultColorScheme, ...renderOptions } = options;
  return render(
    <MantineProvider theme={theme} defaultColorScheme={defaultColorScheme}>
      {ui}
    </MantineProvider>,
    renderOptions,
  );
}
