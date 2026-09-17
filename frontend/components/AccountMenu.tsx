'use client';

import Link from 'next/link';
import { Avatar, Menu, UnstyledButton } from '@mantine/core';
import { useLogout } from './useLogout';
import type { NavDestination } from '@/lib/navLinks';

/** The authenticated visitor's account control: an avatar in the nav bar
 * that opens a `Menu` of their per-account destinations plus "Log out".
 *
 * Why this exists at all is a layout fact, measured rather than guessed.
 * Before it, the authenticated bar carried the display name as visible
 * text plus a full "Log out" `Button` plus two per-account links, and at
 * 1440x900 that came to 1112px of content inside a 1100px container — so
 * the bar wrapped onto a second row and the header grew from 61px to
 * 104px in Chromium, while Firefox's narrower text metrics left the same
 * markup on one row at 63px. (Those four numbers are real measurements of
 * this app against a live backend, not estimates; the regression net that
 * keeps them honest is `e2e/nav.spec.ts`, which asserts the rendered nav
 * height in BOTH engines.) Collapsing name + "Log out" + two links into
 * one ~30px avatar is what makes the anonymous and authenticated bars the
 * same height instead of differing by a whole row.
 *
 * Standalone and importable, taking its `destinations` as a prop: nothing
 * here knows this app's routes, so another surface needing an account
 * menu imports this rather than copying the avatar/menu/logout wiring.
 * `app/layout.tsx` supplies the real list from `lib/navLinks.ts`.
 *
 * `'use client'`, necessarily — a `Menu` is a disclosure with focus
 * management, and "Log out" fires a request. The session is NOT fetched
 * here though: `label` arrives already resolved from the server, the same
 * split `AuthStatus` (its only caller) uses for `session`, so this adds
 * no client-side session round trip. */
export function AccountMenu({
  label,
  destinations,
}: {
  /** The visitor's display name, already resolved to something
   * non-blank by `AuthStatus` — this component never sees the raw
   * session and so cannot re-introduce the empty-label bug that
   * `AuthStatus`'s own `?.trim() ||` chain exists to prevent. */
  label: string;
  destinations: readonly NavDestination[];
}) {
  const { logout, loggingOut } = useLogout();

  return (
    <Menu
      position="bottom-end"
      shadow="md"
      width={220}
      // "required for navigation menus following WAI-ARIA disclosure
      // pattern" (Mantine's own prop doc). This menu is mostly links, so
      // Tab must walk them rather than skipping straight out of the
      // dropdown the way the default `-1` would.
      menuItemTabIndex={0}
      // Mantine's initial-focus placeholder is a `<div role="presentation"
      // tabindex="-1">` rendered as the FIRST child of the `role="menu"`
      // dropdown, and axe's `aria-required-children` (serious) fires on
      // it: a menu's children must be menuitems, and a presentational div
      // is not one. Confirmed against this component in
      // e2e/accessibility.spec.ts's opened-menu case.
      //
      // Safe to switch off here specifically because of
      // `menuItemTabIndex={0}` above: the placeholder exists to stop
      // keyboard focus jumping past a dropdown whose items are all
      // `tabindex="-1"`, and this menu's items are all in the tab order
      // already, so there is nothing for focus to jump over.
      withInitialFocusPlaceholder={false}
    >
      <Menu.Target>
        {/* The accessible name carries the display name that used to sit
            in the bar as visible text: dropping it from the bar is where
            most of the width saving comes from, but it must not vanish
            from the accessibility tree with it. The visible initials
            (Avatar's `name`) are the sighted equivalent, and the full
            name is repeated as the dropdown's own `Menu.Label`. */}
        <UnstyledButton aria-label={`Account menu for ${label}`}>
          {/* 28px, the height the nav's `ActionIcon`s render at, because
              the bar's height is the tallest control in it: at Mantine's
              default 30px this one control made the authenticated bar
              63px against the anonymous bar's 61px — the very "same
              height for everyone" property the account menu exists to
              establish, lost by two pixels. */}
          <Avatar name={label} color="initials" size={28} radius="xl" />
        </UnstyledButton>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Label>{label}</Menu.Label>
        {destinations.map((destination) => (
          <Menu.Item key={destination.href} component={Link} href={destination.href}>
            {destination.label}
          </Menu.Item>
        ))}
        <Menu.Divider />
        {/* `closeMenuOnClick={false}`: the request is in flight and the
            item shows a disabled/busy state while it is, so closing the
            dropdown out from under the press would hide the only
            feedback there is. `useLogout`'s `router.refresh()` re-renders
            this whole control away (into the anonymous "Log in" link)
            once the cookie is gone, which is what actually dismisses it. */}
        <Menu.Item onClick={logout} disabled={loggingOut} closeMenuOnClick={false}>
          Log out
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  );
}
