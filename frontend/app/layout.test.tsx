import { readFileSync } from 'node:fs';
import { describe, it, expect, vi } from 'vitest';
import { viewport, metadata } from './layout';

// This file imports `app/layout.tsx`, which imports `@/lib/api` -- whose
// module scope reads `next/headers`. There is no Next request context in a
// unit test, and nothing here should reach the network at all.
vi.mock('@/lib/api', () => ({
  getDataFreshness: vi.fn(),
  getSessionOrLoggedOut: vi.fn(),
  getMyGroups: vi.fn(),
  getChatbotAccess: vi.fn(),
  LOGGED_OUT_SESSION: { authenticated: false, id: null, email: null, name: null },
}));

describe('viewport.themeColor', () => {
  it('pairs the light-scheme white background with the dark-scheme #242424 body colour', () => {
    // Only asserts the themeColor field specifically -- not a full-object
    // equality check on `viewport` -- so this test doesn't break if a
    // sibling feature (docs/superpowers/plans/2026-09-01-dynamic-color-scheme-meta.md)
    // has also added a `colorScheme` field to the same object.
    expect(viewport.themeColor).toEqual([
      { media: '(prefers-color-scheme: light)', color: '#ffffff' },
      { media: '(prefers-color-scheme: dark)', color: '#242424' },
    ]);
  });
});

describe('viewport.colorScheme', () => {
  it('defaults to light for the pre-hydration SSR render', () => {
    // No route in this app defines its own `viewport` export (confirmed by
    // grep against frontend/app/), so this root-level default is the value
    // Next actually renders for every page. Note this is now true of
    // `viewport` ONLY: several routes do export their own `metadata`/
    // `generateMetadata` for link previews, and those override this file's
    // `metadata` per-field -- but `viewport` is a separate export Next
    // resolves independently, and nothing overrides it. 'light' matches
    // ThemeToggle's own pre-mount fallback (useComputedColorScheme('light')),
    // not a new, third opinion about what "unknown" means.
    expect(viewport.colorScheme).toBe('light');
  });
});

describe('metadata.appleWebApp', () => {
  it('sets statusBarStyle to black-translucent and explicitly disables capable -- no title', () => {
    // Exact-shape check, not just a `.statusBarStyle` field check: this is
    // the one place this plan's Global Constraints must hold structurally
    // -- `title` must never be added alongside this, and `capable` must be
    // explicitly `false` (not omitted): Next's own `resolveAppleWebApp`
    // defaults `capable` to `true` whenever `appleWebApp` is set at all
    // and no `capable` key is present, which would silently emit the
    // discouraged `mobile-web-app-capable` tag this plan's Global
    // Constraints reject -- omitting the key is not equivalent to
    // rejecting the tag here.
    expect(metadata.appleWebApp).toEqual({ capable: false, statusBarStyle: 'black-translucent' });
  });
});

describe('metadata: what the root layout deliberately omits', () => {
  it('has no openGraph or twitter of its own, which is why every page repeats its title into both', () => {
    // The premise the per-page metadata exports rest on, asserted rather
    // than left as prose in this file's own comment. Next merges page
    // metadata into a layout's PER FIELD, not per-object: because there is
    // no `openGraph` object here to inherit from, a page that sets only
    // `title`/`description` emits no `og:title` at all. That is exactly
    // why `/`, `/incidents`, `/trains`, `/stations` and the five detail
    // routes each repeat the same pair of strings into `openGraph` and
    // `twitter` instead of relying on inheritance. If a site-wide
    // `openGraph` is ever added here, that reasoning changes and those
    // nine exports should be revisited together.
    expect(metadata.openGraph).toBeUndefined();
    expect(metadata.twitter).toBeUndefined();
  });

  it('sets no metadataBase, because no route emits a metadata URL needing one', () => {
    // `metadataBase` exists solely to resolve RELATIVE metadata URLs into
    // absolute ones. No route in this app emits `openGraph.images`,
    // `openGraph.url` or `alternates.canonical`, so there is nothing to
    // resolve and no configured public base URL to point one at. This
    // pins that as a decision rather than an oversight -- add one here,
    // and only here, the day a route gains an OG image or a canonical URL.
    expect(metadata.metadataBase).toBeUndefined();
  });
});

describe('page content landmark', () => {
  // RootLayout renders <html>/<body>, which @testing-library/react can't
  // mount into a <div> container, so this asserts on the source rather
  // than the DOM -- the same tactic app/globals.test.ts uses for CSS
  // rules that only exist at the stylesheet level. The live-DOM check for
  // this lives in e2e/accessibility.spec.ts instead.
  it('renders page content inside a <main> landmark, not a bare Container div', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<Container\s+component="main"/);
  });
});

describe('backend reachability threading', () => {
  // RootLayout renders <html>/<body> and cannot be mounted by
  // @testing-library/react (same constraint the <main> landmark test
  // above documents), so this asserts on the source -- the established
  // tactic in this file and in app/globals.test.ts. The behavioural
  // coverage lives in ConnectivityMonitor.test.tsx and e2e.
  it('passes a backendReachable boolean derived from the freshness fetch', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/backendReachable = true/);
    expect(source).toMatch(/backendReachable = false/);
  });

  it('passes the freshness straight into the nav bar rather than streaming it', () => {
    // Correction 1's load-bearing structural change: a streamed freshness
    // fetch resolves after RootLayout has returned, so its outcome could
    // never reach a sibling. It is therefore awaited and handed to
    // <AppNavBar> as a plain prop -- including on the Suspense fallback
    // path, which renders the same bar logged-out.
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<AppNavBar session=\{LOGGED_OUT_SESSION\} freshness=\{freshness\} \/>/);
    expect(source).toMatch(/<NavBarWithSession freshness=\{freshness\} \/>/);
  });

  it('still streams the session check behind its own Suspense boundary', () => {
    // The session fetch must NOT join the awaited pair above: unlike
    // freshness it is not a connectivity oracle, and blocking first paint
    // on it would hand every route the session endpoint's latency.
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<Suspense fallback=\{<AppNavBar[^>]*>\}>\s*<NavBarWithSession/);
  });

  it('makes exactly one getSessionOrLoggedOut() call for the whole nav', () => {
    // Was two -- one for the account control, one for the "Groups" link
    // -- and `getSession()` (which this wraps) is `cache: 'no-store'`, so
    // that was two real round trips per page load. The account menu
    // collapsed both decisions into one component; this pins the saving.
    // No longer a bare `getSession().catch(...)` -- that fallback is now
    // centralized in `getSessionOrLoggedOut()` itself (`lib/api.ts`), which
    // logs a genuine failure (network error, timeout, 5xx -- never a
    // confirmed "not logged in") instead of silently rendering the same nav
    // as someone who really is logged out. See lib/api.test.ts for that
    // behaviour's own coverage.
    // Matches the invoking CALL specifically (immediately followed by the
    // `,` that separates it from the `getChatbotAccess()` leg inside
    // `Promise.all([...])`), not the bare identifier: this file's own doc
    // comments above also mention `getSessionOrLoggedOut()` in prose
    // without invoking it.
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source.match(/getSessionOrLoggedOut\(\),/g)).toHaveLength(1);
  });

  // Review §3.1.3: /chat was undiscoverable -- getChatbotAccess() now rides
  // alongside the session check in the same fetch/Suspense boundary, rather
  // than adding a third sequential wait of its own.
  it('fetches getChatbotAccess() concurrently with getSessionOrLoggedOut(), inside the same Suspense boundary', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    // Both calls sit inside the SAME `Promise.all([...])` -- not a second,
    // separately-awaited call outside it, which would be a sequential wait
    // stacked on top of the session fetch rather than a concurrent one.
    const promiseAllMatch = source.match(/Promise\.all\(\[([\s\S]*?)\]\)/);
    expect(promiseAllMatch).not.toBeNull();
    const body = promiseAllMatch![1];
    expect(body).toMatch(/getSessionOrLoggedOut\(\)/);
    expect(body).toMatch(/getChatbotAccess\(\)/);
  });
});

describe('group summaries provider threading', () => {
  // Same source-assertion tactic as the two `describe` blocks above, and
  // for the same reason: RootLayout renders <html>/<body> and awaits
  // getDataFreshness()/getMyGroups() before returning, so it can't be
  // mounted by @testing-library/react. Behavioural coverage for the
  // provider/hook themselves lives in lib/useGroupSummaries.test.tsx; the
  // fetch-race regression coverage for the three consumers
  // (TrackThisTrainButton/TrackTrainForm/AddToGroupButton) lives in their
  // own test files.
  it('fetches getMyGroups() and wraps the shell in GroupSummariesProvider', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/getMyGroups\(/);
    expect(source).toMatch(/<GroupSummariesProvider groups=\{groups\}>/);
  });

  // The whole point of this fix: the fetch must not be serialized after
  // getDataFreshness (which RootLayout already awaits before returning any
  // HTML) -- it has to be already in flight so the two round trips to the
  // same in-cluster `api` service overlap instead of stacking.
  it('starts the getMyGroups() fetch before awaiting getDataFreshness, not after', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    const groupsCallIndex = source.indexOf('getMyGroups(');
    const freshnessAwaitIndex = source.indexOf('await getDataFreshness(');
    expect(groupsCallIndex).toBeGreaterThan(-1);
    expect(freshnessAwaitIndex).toBeGreaterThan(-1);
    expect(groupsCallIndex).toBeLessThan(freshnessAwaitIndex);
  });
});

// The nav's own link/landmark/breakpoint behaviour is no longer asserted
// here by reading this file's source: it moved into
// `components/AppNavBar.tsx`, which is a plain synchronous component and
// so can be RENDERED and asserted against real DOM instead. See
// components/AppNavBar.test.tsx (bar links, drawer contents, the account
// menu swap), components/AppNavDrawer.test.tsx and
// components/AccountMenu.test.tsx. What stays here is only what is
// genuinely about the layout module itself: its metadata/viewport
// exports and the source-level threading assertions above, both of which
// exist because RootLayout renders <html>/<body> and awaits its fetches,
// so @testing-library/react cannot mount it.
