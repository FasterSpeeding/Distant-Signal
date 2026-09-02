import '@/app/globals.css';
import { Suspense } from 'react';
import { ColorSchemeScript, mantineHtmlProps, Group, Text, Box, Container } from '@mantine/core';
import Link from 'next/link';
import type { Metadata, Viewport } from 'next';
import { ThemeToggle } from '@/components/ThemeToggle';
import { PrideToggle } from '@/components/PrideToggle';
import { TextLink } from '@/components/TextLink';
import { DataFreshnessInfo } from '@/components/DataFreshnessInfo';
import { AuthStatus } from '@/components/AuthStatus';
import { AutoRefresh } from '@/components/AutoRefresh';
import { ColorSchemeMeta } from '@/components/ColorSchemeMeta';
import { ServiceWorkerRegister } from '@/components/ServiceWorkerRegister';
import { OpenDataAttribution } from '@/components/OpenDataAttribution';
import { AppMantineProvider } from '@/components/AppMantineProvider';
import { ConnectivityMonitor } from '@/components/ConnectivityMonitor';
import { getDataFreshness, getSession } from '@/lib/api';
import type { DataFreshness } from '@/lib/types';

export const metadata: Metadata = {
  title: 'Distant Signal',
  description:
    'A personal UK rail companion: TfL-style line status, live train tracking, and ticket/Delay-Repay support — with first-class handling of operators whose routes share trunk track, so an incident is only ever flagged on the lines it actually affects.',
  // `capable: false` is required, not redundant: Next's own
  // `resolveAppleWebApp` (node_modules/next/dist/lib/metadata/resolvers/
  // resolve-basics.js) defaults `capable` to `true` whenever `appleWebApp`
  // is set at all and the caller doesn't include a `capable` key -- so
  // omitting it here would *still* emit a `mobile-web-app-capable` meta
  // tag, the exact discouraged/rejected tag this plan's Global Constraints
  // say to never add. Verified empirically against a running `next start`
  // server this session: without this line, `mobile-web-app-capable`
  // appeared in the rendered <head> even though only `statusBarStyle` was
  // set.
  appleWebApp: {
    capable: false,
    statusBarStyle: 'black-translucent',
  },
};

// `colorScheme`'s 'light' is the same deterministic pre-mount fallback
// ThemeToggle.tsx's own useComputedColorScheme('light') call already uses —
// the server can't know a visitor's stored preference (see ThemeToggle.tsx's
// own comment on this), so this agrees with the one opinion the rest of the
// page already commits to rather than inventing a second one. ColorSchemeMeta
// (mounted in RootLayout below) keeps the resulting <meta name="color-scheme">
// tag's content in sync with the actually-resolved theme after mount. See
// docs/superpowers/specs/2026-09-01-dynamic-color-scheme-meta-design.md.
export const viewport: Viewport = {
  colorScheme: 'light',
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#242424' },
  ],
};

// Takes `freshness` as a prop rather than fetching it itself. The fetch
// moved up into RootLayout (below) because its *success or failure* is
// this app's backend-reachability signal -- and a fetch inside a
// <Suspense> boundary resolves after RootLayout has already returned its
// JSX, so RootLayout could never read the outcome to pass to a sibling.
// See docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md
// Decision 1 and its implementation plan's Correction 1.
//
// The cost, stated plainly: this nav-bar tooltip no longer streams in --
// RootLayout awaits it before emitting any HTML. Acceptable because the
// call is against the same in-cluster `api` service every page already
// awaits for its own content, and because in the failure case (the one
// this whole design exists for) we specifically need the outcome before
// first paint. AuthNavItem below deliberately keeps its own <Suspense>:
// it is not a connectivity oracle.
export function DataFreshnessNavItem({ freshness }: { freshness: DataFreshness }) {
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

// Reclassified from Tier 3 (hidden entirely when logged out) to
// always-visible, per
// docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
// Decision 6 -- a deliberate, named reversal of
// docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md's
// Decision 4, which chose "hidden entirely" specifically because at the
// time an anonymous click would have resolved to a bare inline sentence
// with nothing else on the page. Now that `/track/mine`'s own existing
// `getMyTrackedTrains()` null-on-401 gate (unchanged -- see
// `app/track/mine/page.tsx`) opens a real, actionable `LoginPromptModal`
// instead, "dead weight in the nav bar" no longer describes what a
// logged-out click produces, so this link is worth advertising rather
// than hiding.
//
// No `getSession()` call here any more, and no `Suspense` wrapper needed
// at the call site below -- the real gating already lives entirely on
// `/track/mine`'s own page, which has no id in its path to disambiguate
// (same reasoning that page's own doc comment already gives for not
// needing a second `getSession()` call of its own). Adding a second,
// client-side session check here just to decide what to render would be
// duplicate plumbing for a decision this nav item no longer needs to
// make.
//
// Labelled "My Trains & Tickets," not "My Tracked Trains," now that
// `/track/mine` is the single merged page for both (Part B of the
// upload-first ticket-tracking plan).
export function TrackedTrainsNavItem() {
  return <TextLink href="/track/mine">My Trains &amp; Tickets</TextLink>;
}

const UNAVAILABLE_FRESHNESS: DataFreshness = {
  stations: null,
  tocs: null,
  incidents: null,
  tfl: null,
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // A root layout has no route-level `error.tsx` boundary (that only
  // catches errors in child segments), so an uncaught fetch failure here
  // would take down every page rather than just one -- fall back to an
  // all-"never fetched" state instead. Unchanged in substance from the
  // previous `.catch()` on this same call; the only addition is that we
  // now also record *whether* it fell back, which is the
  // backend-reachability signal ConnectivityMonitor debounces.
  let freshness: DataFreshness;
  let backendReachable: boolean;
  try {
    freshness = await getDataFreshness();
    backendReachable = true;
  } catch {
    freshness = UNAVAILABLE_FRESHNESS;
    backendReachable = false;
  }
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript defaultColorScheme="auto" />
      </head>
      <body>
        <AppMantineProvider>
          {/* Wraps the whole shell rather than only <Container
              component="main">: the banner's fixed positioning is then not
              constrained by the content container, and app/error.tsx --
              which renders inside <Container component="main"> below --
              ends up a descendant, which is what lets it read the context
              and auto-recover. */}
          <ConnectivityMonitor
            backendReachable={backendReachable}
            observedAt={new Date().toISOString()}
          >
            <AutoRefresh />
            <ColorSchemeMeta />
            {/* RootLayout is a Server Component and re-executes on every
                navigation and every AutoRefresh-triggered router.refresh() --
                a fresh ISO timestamp here is what lets
                ServiceWorkerRegister record "last successful load" purely
                from receiving a new prop value; see that component's own
                doc comment. */}
            <ServiceWorkerRegister loadedAt={new Date().toISOString()} />
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
                    <TrackedTrainsNavItem />
                    <DataFreshnessNavItem freshness={freshness} />
                    <ThemeToggle />
                    <PrideToggle />
                    <Suspense fallback={<Text size="sm" c="dimmed">Log in</Text>}>
                      <AuthNavItem />
                    </Suspense>
                  </Group>
                </Group>
              </Container>
            </Box>
            {/* `component="main"`: Mantine's Container renders a plain
                <div> by default, which left every page's actual content
                outside any landmark -- axe's `landmark-one-main` fired on
                every route tested, and `region` fired once per unlandmarked
                node (487 on /lines alone). See
                docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md.
                The nav (:144) and footer (OpenDataAttribution.tsx) were
                already landmarked; only the middle was not. Polymorphic
                `component` swaps the tag only -- size/px/class output is
                unchanged. */}
            <Container component="main" size="lg" px={0}>
              {children}
            </Container>
            <OpenDataAttribution />
          </ConnectivityMonitor>
        </AppMantineProvider>
      </body>
    </html>
  );
}
