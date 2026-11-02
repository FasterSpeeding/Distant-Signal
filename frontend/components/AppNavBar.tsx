import { Suspense } from 'react';
import Link from 'next/link';
import { Box, Container, Group, Text } from '@mantine/core';
import { AppNavDrawer } from './AppNavDrawer';
import { AuthStatus } from './AuthStatus';
import { DataFreshnessInfo } from './DataFreshnessInfo';
import { PreferencesNavIcon } from './PreferencesNavIcon';
import { PrideModeApplier, PrideSparkles } from './PrideMode';
import { PrimaryNavLink } from './PrimaryNavLink';
import { Wordmark } from './Wordmark';
import { navDrawerDestinations, primaryNavDestinations } from '@/lib/navLinks';
import type { DataFreshness, SessionInfo } from '@/lib/types';

/** The site header, lifted out of `app/layout.tsx` so the whole bar can
 * be rendered — and asserted on — as one unit. Server-renderable (no
 * `'use client'`): it takes both the session and the freshness as already
 * resolved props and makes no fetch of its own, so the single
 * `getSession()` call stays in the layout, inside a `<Suspense>`
 * boundary. Its interactive parts (`AppNavDrawer`,
 * `PrideModeApplier`, `DataFreshnessInfo`, `AuthStatus`'s leaves) are imported
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
// A responsive `gap={{ base: 'xs', md: 'lg' }}` silently does nothing:
// Mantine's `Group` types `gap` as a plain `MantineSpacing` and feeds it
// straight into `--group-gap: var(--mantine-spacing-<value>)`, so an object
// renders as `var(--mantine-spacing-[object Object])` and falls back to the
// stylesheet default with no type error. So the gaps are flat ("xs", 10px)
// and the SHAPE carries the responsiveness: the icon controls are their own
// tight group.
//
// The bar must stay one row from `md` (992px) up, logged in or out, in
// Chromium and Firefox; `e2e/nav.spec.ts` asserts it. Round 2 (Oct 2026)
// merged four tracking entries into one "Track" and moved the theme toggle
// to /account/preferences, which bought back enough width for "Plan a
// journey" to sit inline at every width from `md` up. The anonymous bar is
// the binding case only through its "Log in" link; logged in, the account
// menu avatar is narrower.

export function AppNavBar({
  session,
  freshness,
  transitOn = false,
}: {
  session: SessionInfo;
  freshness: DataFreshness;
  /** Whether the api serves any transit network: the bar and the drawer
   * then both list "Trams and metros". Defaults to `false`, i.e. "not
   * shown"; `app/layout.tsx` passes it to the `<Suspense>` fallback as well
   * as the resolved bar, so the entry doesn't pop in. */
  transitOn?: boolean;
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
    <Box component="nav" aria-label="Main" className="siteNav">
      <Container size="lg" px={0}>
        {/* `gap="xs"` on a `space-between` row is not about spacing --
            nothing normally sits at the gap -- it is the MINIMUM the two
            halves may be pushed to before the row wraps. Mantine's
            default `md` (16px) is what tipped the 390px bar over its
            container by 5px in Chromium. */}
        <Group justify="space-between" align="center" gap="xs" px="lg" py="md" mih={BAR_MIN_HEIGHT}>
          <Group gap="xs" wrap="nowrap">
            {/* Left of the brand, the conventional position — and the
                reason the burger is not simply folded in with the
                right-hand controls, where it would sit behind three
                icons a thumb has to skip past. */}
            <AppNavDrawer
              destinations={navDrawerDestinations(session.authenticated, transitOn)}
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
            {/* `prideToggle` anchors the decorative sparkle scatter
                (`PrideSparkles`, only there while a pride flag is on). */}
            <span className="prideToggle">
              <Link href="/" data-card-link>
                <Wordmark />
              </Link>
              <PrideSparkles />
            </span>
          </Group>
          {/* `gap="xs"` here and between the links: see the gap note
              above this component. */}
          <Group gap="xs" wrap="nowrap">
            <Group gap="xs" wrap="nowrap" visibleFrom={NAV_BREAKPOINT}>
              {primaryNavDestinations(transitOn).map((destination) => (
                <PrimaryNavLink key={destination.href} {...destination} />
              ))}
            </Group>
            {/* The always-on controls: the freshness readout, plus the
                pride flag applier, which renders nothing. The theme and
                pride controls themselves live on /account/preferences:
                logged out, the gear links there from `md` up (below it the
                drawer does); logged in, the account menu does. */}
            <Group gap="xs" wrap="nowrap">
              <DataFreshnessInfo freshness={freshness} />
              {!session.authenticated && <PreferencesNavIcon visibleFrom={NAV_BREAKPOINT} />}
              <PrideModeApplier />
            </Group>
            {/* This boundary is load-bearing and has nothing to do with
                data fetching: the anonymous branch of `AuthStatus` is
                `LoginControl`, which calls `useSearchParams()` (via
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
            {/* `c="var(--mantine-color-anchor)"`, matching the resolved
                `<LoginControl>`'s own colour (via `TextLink`) -- not
                `c="dimmed"`. Both render the exact same "Log in" string,
                and this boundary is identical on every route, so a
                mismatch here reads as the same text flipping colour
                purely by streaming timing, not anything route-specific
                (review §4.9). See `AppNavBar.test.tsx`'s own regression
                test, which pins the two equal. */}
            <Suspense
              fallback={
                <Text size="sm" c="var(--mantine-color-anchor)">
                  Log in
                </Text>
              }
            >
              <AuthStatus session={session} />
            </Suspense>
          </Group>
        </Group>
      </Container>
    </Box>
  );
}
