import '@/app/globals.css';
import { Suspense } from 'react';
import { ActionIcon, MantineProvider, ColorSchemeScript, mantineHtmlProps, Group, Text, Box, Container } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { ThemeToggle } from '@/components/ThemeToggle';
import { PrideToggle } from '@/components/PrideToggle';
import { TextLink } from '@/components/TextLink';
import { DataFreshnessInfo } from '@/components/DataFreshnessInfo';
import { AuthStatus } from '@/components/AuthStatus';
import { AutoRefresh } from '@/components/AutoRefresh';
import { OpenDataAttribution } from '@/components/OpenDataAttribution';
import { getDataFreshness, getSession } from '@/lib/api';
import { theme } from '@/lib/theme';

export const metadata: Metadata = {
  title: 'Distant Signal',
  description:
    'A personal UK rail companion: TfL-style line status, live train tracking, and ticket/Delay-Repay support — with first-class handling of operators whose routes share trunk track, so an incident is only ever flagged on the lines it actually affects.',
};

// A separate async Server Component (rather than awaiting inline in
// RootLayout) so `<Suspense>` below can stream it in without blocking the
// rest of the shell — this is decorative nav-bar data, not core page
// content, and shouldn't add to every route's time-to-first-byte.
async function DataFreshnessNavItem() {
  // A root layout has no route-level `error.tsx` boundary (that only
  // catches errors in child segments), so an uncaught fetch failure here
  // would take down every page rather than just one — fall back to an
  // all-"never fetched" state instead.
  const freshness = await getDataFreshness().catch(() => ({
    stations: null,
    tocs: null,
    incidents: null,
    tfl: null,
  }));
  return <DataFreshnessInfo freshness={freshness} />;
}

// Same rationale as `DataFreshnessNavItem` immediately above: a separate
// async Server Component so `<Suspense>` can stream the session check in
// without blocking the rest of the shell, and so an uncaught fetch
// failure here (this root layout has no route-level `error.tsx`) can't
// take down every page. Falls back to a logged-out session shape rather
// than rethrowing — an auth-status glitch should degrade to "show the log
// in link", not break navigation for every visitor, logged in or not.
async function AuthNavItem() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  return <AuthStatus session={session} />;
}

// Same rationale as AuthNavItem/DataFreshnessNavItem: a separate async
// Server Component so <Suspense> can stream the session check in without
// blocking the rest of the shell. Unlike those two, this one renders
// nothing at all when logged out (Decision 4 of
// docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md) --
// this is a full nav-bar entry point to a page whose entire content is
// private to the viewer, not a section of an already-public page (the
// TicketPanel pattern), so showing it to every visitor and having it
// always resolve to a login nudge would be dead weight for the common
// case of an anonymous visitor. Guarded with the same .catch() shape as
// AuthNavItem/DataFreshnessNavItem: a root layout has no route-level
// error.tsx, so an unguarded getSession() here could take down every
// page's nav bar on an auth glitch -- the same historical bug class
// already fixed in TicketPanel.tsx, not repeated here.
//
// Labelled "My Trains & Tickets," not "My Tracked Trains," now that
// `/track/mine` is the single merged page for both (Part B of the
// upload-first ticket-tracking plan) -- the separate `MyTicketsNavItem`
// this file used to also render (pointing at the now-redirected
// `/track/tickets`) is gone; one nav entry for one merged page.
export async function TrackedTrainsNavItem() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  if (!session.authenticated) {
    return null;
  }
  return <TextLink href="/track/mine">My Trains &amp; Tickets</TextLink>;
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript defaultColorScheme="auto" />
      </head>
      <body>
        <MantineProvider theme={theme} defaultColorScheme="auto">
          <AutoRefresh />
          {/* No max-width anywhere meant a 1920px viewport put a line's
              name at x≈30, its status badge at x≈870 and its pin at
              x≈1780 — the row stopped being scannable as a row. `lg` is
              1140px. The border stays on a full-bleed Box so the rule still
              spans the window while the nav's contents line up with the
              page content below it. `px={0}`: every page already applies
              its own `p="lg"`, and Container's default `md` inline padding
              on top of that is 40px of gutter on a 390px screen. */}
          <Box
            component="nav"
            style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}
          >
            <Container size="lg" px={0}>
              <Group justify="space-between" px="lg" py="md">
                {/* Plain `<Link>` wrapping Mantine's `Text`, rather than
                    `component={Link}` on a Mantine polymorphic prop: this file
                    is a Server Component, and passing the `Link` component
                    reference into a Mantine `component` prop from a Server
                    Component previously broke `next build`'s Server/Client
                    boundary serialization check (see LineStatusCard fix).
                    `ThemeToggle` below doesn't hit this: it's imported and
                    rendered as a plain JSX element (a Client Component child
                    of this Server Component), not passed as a value into a
                    Mantine `component` prop — a different, safe pattern. */}
                <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
                  {/* `data-site-title` is a pure CSS hook for `globals.css`'s
                      `body[data-pride='true']` rules -- Mantine's `Text`
                      renders no stable class of its own to key off. */}
                  <Text fw={700} data-site-title>
                    Distant Signal
                  </Text>
                </Link>
                <Group gap="lg">
                  <TextLink href="/lines">All Lines</TextLink>
                  <TextLink href="/stations">Station Lookup</TextLink>
                  <Suspense fallback={null}>
                    <TrackedTrainsNavItem />
                  </Suspense>
                  <Suspense fallback={<ActionIcon variant="subtle" aria-label="Data freshness" disabled loading />}>
                    <DataFreshnessNavItem />
                  </Suspense>
                  <ThemeToggle />
                  <PrideToggle />
                  <Suspense fallback={<Text size="sm" c="dimmed">Log in</Text>}>
                    <AuthNavItem />
                  </Suspense>
                </Group>
              </Group>
            </Container>
          </Box>
          <Container size="lg" px={0}>
            {children}
          </Container>
          <OpenDataAttribution />
        </MantineProvider>
      </body>
    </html>
  );
}
