import { Suspense } from 'react';
import Link from 'next/link';
import { Box, Container, Group, Text } from '@mantine/core';
import { AppNavDrawer } from './AppNavDrawer';
import { AuthStatus } from './AuthStatus';
import { DataFreshnessInfo } from './DataFreshnessInfo';
import { PrideToggle } from './PrideToggle';
import { TextLink } from './TextLink';
import { ThemeToggle } from './ThemeToggle';
import {
  navDrawerDestinations,
  PRIMARY_NAV_DESTINATIONS,
  TRACKED_TRAINS_DESTINATION,
} from '@/lib/navLinks';
import type { DataFreshness, SessionInfo } from '@/lib/types';

/** The site header, lifted out of `app/layout.tsx` so the whole bar can
 * be rendered — and asserted on — as one unit. Server-renderable (no
 * `'use client'`): it takes both the session and the freshness as already
 * resolved props and makes no fetch of its own, so the single
 * `getSession()` call stays in the layout, inside a `<Suspense>`
 * boundary. Its interactive parts (`AppNavDrawer`, `ThemeToggle`,
 * `PrideToggle`, `DataFreshnessInfo`, `AuthStatus`'s leaves) are imported
 * and rendered as plain JSX elements — Client Component children of a
 * Server Component, the safe direction across the boundary.
 *
 * ---------------------------------------------------------------------
 * The two layout problems this shape exists to fix
 * ---------------------------------------------------------------------
 * Both are measured, against a live backend, at the stated viewports.
 *
 * 1. Below `md` the bar was unusable. Ten flat items in one wrapping
 *    `Group` came to a 195px-tall, five-row header at 390px wide — over
 *    a fifth of the viewport, in both Chromium and Firefox, before any
 *    page content. They now live in `AppNavDrawer` behind a burger.
 *
 * 2. At 1440x900 the AUTHENTICATED bar wrapped and the anonymous one did
 *    not, so the header height depended on who was looking: 104px logged
 *    in vs 61px logged out in Chromium. The content measured 1112px
 *    inside a 1100px container — twelve pixels over. Firefox's narrower
 *    text metrics fitted the identical markup on one row (63px), which
 *    is exactly why this is verified in both engines rather than
 *    eyeballed in one; `e2e/nav.spec.ts` is the standing net.
 *
 * `BAR_MIN_HEIGHT` is the "fixed header height" half of fix 2: with the
 * content fitting on one row at every width from 360px up, a floor makes
 * the anonymous and authenticated bars the same height by construction
 * rather than by coincidence. `mih` rather than `h`: a hard height would
 * CLIP if some future label, font stack or locale did overflow, whereas a
 * floor degrades to the old (ugly but readable) two-row bar. 60px is the
 * height the bar already rendered at — `py="md"` either side of a 28px
 * control row — so this pins today's appearance rather than introducing a
 * new one.
 *
 * Note the outer `Group` deliberately does NOT get `wrap="nowrap"`, for
 * the same reason: at 320px (narrower than anything this work targets,
 * but real) the controls genuinely do not fit, and wrapping to a second
 * row beats a horizontally-scrolling page. */
const BAR_MIN_HEIGHT = 60;

/** The one breakpoint the whole nav pivots on: below it the burger and
 * drawer are the only navigation; at and above it the inline links
 * appear and the burger disappears. Passed to `AppNavDrawer` (which
 * derives both its `hiddenFrom` and its close-on-resize media query from
 * it) and used for the matching `visibleFrom` here, so the two halves of
 * the swap cannot drift apart.
 *
 * `md` (992px), not the `sm` (768px) the review first suggested. That is
 * one measurement, not a preference: with this file temporarily switched
 * to `sm` and driven against a live backend at 768px in Chromium, the
 * inline bar measured
 *
 *   logged out : 849px of content in 728px available -> WRAPPED, two rows
 *   logged in  : 728px of content in 728px available -> ZERO px of slack
 *
 * i.e. `sm` does not merely cut it fine, it reproduces the exact defect
 * this component exists to fix, one breakpoint down. At `md` the same
 * content has the slack quoted below. Nothing is lost between 768 and
 * 992: that range gets the drawer, which carries every destination the
 * bar does.
 *
 * RE-VERIFY THIS IN FIREFOX after this plan's Task 1.14 (the
 * font-delivery fix). Every number here is font-metric dependent -- that
 * is the whole reason the original defect showed in one engine and not
 * the other -- and changing how fonts are delivered can move Gecko's text
 * metrics again. `e2e/nav.spec.ts` pins 992 and 991 from both sides in
 * both engines, so a regression fails a test rather than shipping. */
const NAV_BREAKPOINT = 'md';

// A note on the gaps below, because they were measured rather than
// picked, and because the obvious way to write them does not work.
//
// With a flat `gap="lg"` (20px) on everything, the bar measured 355px of
// content inside the 350px available at 390px wide in Chromium -- so the
// phone bar wrapped to two rows by FIVE pixels, while Firefox's narrower
// text fitted the same markup on one. That is the same font-metric trap
// as the 1440px defect this whole component exists to fix, five pixels
// instead of twelve.
//
// The natural fix -- a responsive `gap={{ base: 'xs', md: 'lg' }}` --
// silently does not work: Mantine's `Group` types `gap` as a plain
// `MantineSpacing`, not a `StyleProp<MantineSpacing>`, and feeds it
// straight into `--group-gap: var(--mantine-spacing-<value>)`. An object
// stringifies, so the rendered variable is
// `var(--mantine-spacing-[object Object])` -- an invalid reference that
// falls back to the stylesheet default with no type error, no console
// warning and no visual clue beyond "the gap is not what you asked for".
// Verified against the running app before it was replaced by what is
// below. (`Box` style props such as `px` DO take responsive objects;
// `Group`'s own `gap` is the exception, and it is the one that matters
// here.)
//
// So the gaps are flat, and the SHAPE carries the responsiveness
// instead: the icon cluster is its own tight group. The links keep the
// 20px they have always had.
//
// THE slack figures for this layout, measured in one pass against a live
// backend in Chromium (the wider-measuring engine), quoted here and
// nowhere else so there is one copy to keep true. The binding case is the
// ANONYMOUS bar: it is the widest arrangement, because it still carries
// "My Trains & Tickets" inline where an authenticated visitor reaches it
// through the account menu.
//
//   viewport   available   used (anon)   slack (anon)   slack (logged in)
//   390px          350px        325px           25px                43px
//   992px          952px        910px           42px               224px
//   1440px        1100px        910px          190px               372px
//
// Every row is a single row -- that is asserted, not assumed, by
// e2e/nav.spec.ts in both engines.

export function AppNavBar({
  session,
  freshness,
  chatAllowed = false,
}: {
  session: SessionInfo;
  freshness: DataFreshness;
  /** Whether `getChatbotAccess()` resolved to `'allowed'` for this
   * visitor (review §3.1.3) -- fetched alongside `session` in
   * `app/layout.tsx`'s `NavBarWithSession`, inside the same `<Suspense>`
   * boundary, so it costs no extra round trip on top of the one this bar
   * already waits on. Defaults to `false` (no Chat nav item) so the
   * `<Suspense>` fallback in `app/layout.tsx` -- which renders this bar
   * with only `session`/`freshness` supplied -- degrades to "not shown
   * yet" rather than needing a third prop threaded through it too. */
  chatAllowed?: boolean;
}) {
  return (
    // No max-width anywhere meant a 1920px viewport put a line's name at
    // x≈30, its status badge at x≈870 and its pin at x≈1780 — the row
    // stopped being scannable as a row. `lg` is 1140px. The border stays
    // on a full-bleed Box so the rule still spans the window while the
    // nav's contents line up with the page content below it. `px={0}`:
    // every page already applies its own `p="lg"`, and Container's
    // default `md` inline padding on top of that is 40px of gutter on a
    // 390px screen.
    <Box component="nav" style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}>
      <Container size="lg" px={0}>
        {/* `gap="xs"` on a `space-between` row is not about spacing --
            nothing normally sits at the gap -- it is the MINIMUM the two
            halves may be pushed to before the row wraps. Mantine's
            default `md` (16px) is what tipped the 390px bar over its
            container by 5px in Chromium. */}
        <Group
          justify="space-between"
          align="center"
          gap="xs"
          px="lg"
          py="md"
          mih={BAR_MIN_HEIGHT}
        >
          <Group gap="xs" wrap="nowrap">
            {/* Left of the brand, the conventional position — and the
                reason the burger is not simply folded in with the
                right-hand controls, where it would sit behind three
                icons a thumb has to skip past. */}
            <AppNavDrawer
              destinations={navDrawerDestinations(session.authenticated, chatAllowed)}
              hiddenFrom={NAV_BREAKPOINT}
            />
            {/* Plain `<Link>` wrapping Mantine's `Text`, rather than
                `component={Link}` on a Mantine polymorphic prop: this
                file is a Server Component, and passing the `Link`
                component reference into a Mantine `component` prop from a
                Server Component previously broke `next build`'s
                Server/Client boundary serialization check (see
                LineStatusCard fix). The Client Components rendered around
                it don't hit this: they're imported and rendered as plain
                JSX elements, not passed as values into a Mantine
                `component` prop — a different, safe pattern. (Inside
                `AppNavDrawer`, which IS a Client Component,
                `component={Link}` is fine and is used.) */}
            <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
              {/* `data-site-title` is a pure CSS hook for `globals.css`'s
                  `body[data-pride='true']` rules -- Mantine's `Text`
                  renders no stable class of its own to key off. */}
              <Text fw={700} data-site-title>
                Distant Signal
              </Text>
            </Link>
          </Group>
          <Group gap="md" wrap="nowrap">
            <Group gap="lg" wrap="nowrap" visibleFrom={NAV_BREAKPOINT}>
              {PRIMARY_NAV_DESTINATIONS.map((destination) => (
                <TextLink key={destination.href} href={destination.href}>
                  {destination.label}
                </TextLink>
              ))}
              {/* Inline for anonymous visitors only. It is deliberately
                  always-reachable (see `TRACKED_TRAINS_DESTINATION`'s own
                  doc comment for the decision and the spec that reversed
                  an earlier one), and an anonymous visitor has no account
                  menu to reach it through — so dropping it from the bar
                  wholesale would have quietly un-done that decision for
                  every logged-out desktop visitor. A logged-in visitor
                  gets it in the account menu instead, which is part of
                  what buys the authenticated bar its missing 12px. */}
              {!session.authenticated && (
                <TextLink href={TRACKED_TRAINS_DESTINATION.href}>
                  {TRACKED_TRAINS_DESTINATION.label}
                </TextLink>
              )}
            </Group>
            {/* The three always-on icon controls, grouped tightly: they
                are one cluster of same-sized square buttons rather than
                three unrelated items, and the 30px this saves over
                spacing them like links is most of the phone bar's
                remaining slack. */}
            <Group gap="xs" wrap="nowrap">
              <DataFreshnessInfo freshness={freshness} />
              <ThemeToggle />
              <PrideToggle />
            </Group>
            {/* This boundary is load-bearing and has nothing to do with
                data fetching: the anonymous branch of `AuthStatus` is
                `LoginLink`, which calls `useSearchParams()` (via
                `useLoginHref`, to carry a `return_to` through the OIDC
                redirect). On a statically prerendered route that call
                must sit under a `<Suspense>` or `next build` fails the
                whole export with "useSearchParams() should be wrapped in
                a suspense boundary" — verified for real against
                `/chat/callback`, this app's one prerendered route that
                renders the shell.
                `app/layout.tsx`'s own boundary around the session fetch
                does NOT cover this: that one's FALLBACK renders this bar
                too, and a fallback is not itself inside a boundary. So
                the containment has to live here, where both paths pass
                through it. */}
            <Suspense fallback={<Text size="sm" c="dimmed">Log in</Text>}>
              <AuthStatus session={session} chatAllowed={chatAllowed} />
            </Suspense>
          </Group>
        </Group>
      </Container>
    </Box>
  );
}
