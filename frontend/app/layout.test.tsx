import { readFileSync } from 'node:fs';
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DataFreshnessNavItem, TrackedTrainsNavItem, viewport, metadata } from './layout';

// This file imports `app/layout.tsx`, which imports `@/lib/api` -- whose
// module scope reads `next/headers`. There is no Next request context in a
// unit test, and none of the cases here should reach the network at all
// (DataFreshnessNavItem is now a pure prop-taking component; that is
// precisely what the first case below asserts).
vi.mock('@/lib/api', () => ({
  getDataFreshness: vi.fn(),
  getSession: vi.fn(),
}));

describe('TrackedTrainsNavItem', () => {
  it('renders "My Trains & Tickets" unconditionally, pointing at /track/mine', () => {
    // No session check here any more (Decision 6 of
    // docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md) --
    // the real login gate lives entirely on /track/mine's own page now,
    // covered by app/track/mine/page.test.tsx instead.
    renderWithMantine(<TrackedTrainsNavItem />);
    expect(screen.getByRole('link', { name: 'My Trains & Tickets' })).toHaveAttribute('href', '/track/mine');
  });
});

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
    // No route in this app defines its own viewport/metadata export
    // (confirmed by grep against frontend/app/ — this worktree's plan doc
    // for this feature cites the same check), so this root-level default
    // is the value Next actually renders for every page. 'light' matches
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

describe('DataFreshnessNavItem', () => {
  it('renders the freshness it is given, without fetching', async () => {
    const { getDataFreshness } = await import('@/lib/api');
    renderWithMantine(
      <DataFreshnessNavItem freshness={{ stations: null, tocs: null, incidents: null, tfl: null }} />,
    );
    expect(screen.getByRole('button', { name: 'Data freshness' })).toBeInTheDocument();
    // The whole point of Correction 1: this component no longer owns the
    // fetch, so it must not perform one.
    expect(getDataFreshness).not.toHaveBeenCalled();
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

  it('no longer wraps the freshness nav item in a Suspense boundary', () => {
    // Correction 1's load-bearing structural change: a streamed freshness
    // fetch resolves after RootLayout has returned, so its outcome could
    // never reach a sibling. AuthNavItem's own Suspense must survive.
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<DataFreshnessNavItem freshness=\{freshness\} \/>/);
    expect(source).toMatch(/<Suspense fallback=\{<Text size="sm" c="dimmed">Log in<\/Text>\}>/);
  });
});
