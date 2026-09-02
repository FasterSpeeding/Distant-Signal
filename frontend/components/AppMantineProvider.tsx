'use client';

import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';

/** Thin `'use client'` wrapper around Mantine's `MantineProvider` that
 * imports `theme` itself, rather than receiving it as a prop from
 * `app/layout.tsx` (a Server Component).
 *
 * `theme` (`lib/theme.ts`) gained a `variantColorResolver` function as
 * part of fixing the filled-surface contrast findings in
 * docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md.
 * `app/layout.tsx` used to pass `theme={theme}` straight into
 * `MantineProvider` -- fine while `theme` was a plain, serializable
 * object (`{ primaryColor: 'grape' }`), but Next's RSC boundary refuses to
 * serialize a *function* passed as a prop from a Server Component into a
 * Client Component ("Functions cannot be passed directly to Client
 * Components..."), which broke `next build` the moment `theme` gained
 * `variantColorResolver`. Same category of failure `TextLink.tsx`'s own
 * comment describes for the `Link`-into-`component`-prop case, just on a
 * different prop.
 *
 * Importing `theme` as a plain module here, inside client code, sidesteps
 * serialization entirely: only `children` (a JSX tree, which the RSC
 * protocol threads through specially) crosses the boundary, not `theme`
 * itself. `defaultColorScheme="auto"` is hardcoded rather than threaded
 * through as a prop -- `app/layout.tsx` never varied it per-request. */
export function AppMantineProvider({ children }: { children: React.ReactNode }) {
  return (
    <MantineProvider theme={theme} defaultColorScheme="auto">
      {children}
    </MantineProvider>
  );
}
