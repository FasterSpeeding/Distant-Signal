'use client';

import { useEffect } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { Burger, Drawer, NavLink, Stack, useMantineTheme } from '@mantine/core';
import type { MantineBreakpoint } from '@mantine/core';
import { useDisclosure, useMediaQuery } from '@mantine/hooks';
import type { NavDestination } from '@/lib/navLinks';

/** The app's small-screen navigation: a `Burger` in the bar that opens a
 * `Drawer` of full-width, touch-sized links.
 *
 * Standalone and importable on purpose. It takes its destinations as a
 * prop and hard-codes nothing about this app's routes, so a later surface
 * that needs the same disclosure (a different shell, a sub-navigation)
 * imports this rather than growing a second copy of the
 * burger/disclosure/close-on-navigate wiring. `app/layout.tsx` supplies
 * the real list via `lib/navLinks.ts`.
 *
 * `'use client'` because all three of the behaviours here are
 * client-only: the open/closed state, the close-on-navigate effect, and
 * the media query that closes the drawer when the viewport grows past the
 * breakpoint. The session it needs (to decide whether "Groups" is in the
 * list) is NOT fetched here — it arrives already resolved, in the
 * `destinations` prop, exactly the way `AuthStatus` takes `session` as a
 * prop rather than fetching it. That keeps the one `getSession()` call in
 * `app/layout.tsx`, inside a `<Suspense>` boundary, instead of adding a
 * client-side session round trip on every page load.
 *
 * `hiddenFrom` is a prop rather than a hard-coded `'sm'`: the same value
 * has to be applied by the CALLER to whatever it shows instead above the
 * breakpoint (`visibleFrom` on the inline links), and the effect below
 * also derives its media query from it, so all three agree by
 * construction instead of by three separate literals staying in sync. */
export function AppNavDrawer({
  destinations,
  hiddenFrom = 'md',
  title = 'Menu',
  burgerLabel = 'Navigation menu',
}: {
  destinations: readonly NavDestination[];
  /** The breakpoint at and above which the burger disappears. The caller
   * must pair this with `visibleFrom={hiddenFrom}` on whatever it renders
   * in the bar instead. */
  hiddenFrom?: MantineBreakpoint;
  /** The drawer's heading, which also names its dialog for assistive tech. */
  title?: string;
  /** The burger's accessible name. Deliberately not "Open navigation
   * menu": the same button also closes the drawer, and `aria-expanded`
   * below is what announces which of the two a press will do — a name
   * that says "Open" would contradict it while the drawer is open. */
  burgerLabel?: string;
}) {
  const [opened, { toggle, close }] = useDisclosure(false);
  const pathname = usePathname();
  const theme = useMantineTheme();
  // Derived from `hiddenFrom` rather than written out again, so the
  // "burger is gone" breakpoint and the "close the drawer" breakpoint
  // cannot drift apart. `useMediaQuery` returns `undefined` before mount
  // (and in a non-matchMedia environment), which is falsy — so the
  // pre-mount render simply never closes anything.
  const aboveBreakpoint = useMediaQuery(`(min-width: ${theme.breakpoints[hiddenFrom]})`);

  // A soft navigation does not unmount the root layout, so without this
  // the drawer would stay open on top of the page the visitor just asked
  // for. Keyed on the pathname rather than on each link's `onClick` so it
  // also covers a back/forward navigation made while the drawer is open.
  useEffect(() => {
    close();
  }, [pathname, close]);

  // The burger is hidden above the breakpoint, but hiding it does not
  // close an already-open drawer — resize a phone-width window to desktop
  // with the drawer open and you would be left with a modal overlay and
  // no visible control that opened it.
  useEffect(() => {
    if (aboveBreakpoint) {
      close();
    }
  }, [aboveBreakpoint, close]);

  return (
    <>
      <Burger
        opened={opened}
        onClick={toggle}
        hiddenFrom={hiddenFrom}
        size="sm"
        aria-label={burgerLabel}
        aria-expanded={opened}
      />
      {/* `size="xs"` (a narrow left drawer) rather than the default
          `md`: the contents are a single column of short labels, and a
          drawer that covers the whole phone screen reads as a page
          navigation rather than as a panel over the page it came from.

          Deliberately NOT given `component="nav"`/`role="navigation"`:
          Mantine portals the drawer to `document.body`, so it is not
          inside the bar's own `<nav>` landmark, and a second unnamed
          navigation landmark is exactly what axe's `landmark-unique`
          rule fires on — which `e2e/accessibility.spec.ts` runs for real
          against an opened drawer. The dialog's own `title` already
          names it. */}
      <Drawer opened={opened} onClose={close} title={title} size="xs" padding="md">
        <Stack gap={0}>
          {destinations.map((destination) => (
            // Mantine's `NavLink`, not this app's `TextLink`: a drawer row
            // is a touch target, and `NavLink` renders a full-width,
            // padded block (comfortably past the 44px minimum) instead of
            // a bare inline run of text a thumb has to hit exactly.
            <NavLink
              key={destination.href}
              component={Link}
              href={destination.href}
              label={destination.label}
              // Marks the row for the page you are already on. Exact
              // match, not `startsWith`: `/` would otherwise light up on
              // every route, and `/track/mine` is a leaf anyway.
              active={pathname === destination.href}
            />
          ))}
        </Stack>
      </Drawer>
    </>
  );
}
