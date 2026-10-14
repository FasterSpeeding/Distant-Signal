import { readFileSync } from 'node:fs';
import { beforeEach, describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AppNavBar } from './AppNavBar';
import { PLAN_JOURNEY_DESTINATION, PREFERENCES_DESTINATION, PRIMARY_NAV_DESTINATIONS } from '@/lib/navLinks';
import type { DataFreshness, SessionInfo } from '@/lib/types';

// LoginControl / AccountMenu / AppNavDrawer all reach for App Router hooks
// that throw outside a real Next.js tree -- same stub as the rest of this
// directory's tests.
// Mutable so the active-state cases below can put the visitor on a page.
let pathname = '/';
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => pathname,
  useSearchParams: () => new URLSearchParams(''),
}));

const freshness: DataFreshness = {
  stations: null,
  tocs: null,
  incidents: null,
  tfl: null,
  schedule_feed: null,
  corpus: null,
};

const loggedOut: SessionInfo = { authenticated: false, id: null, username: null, name: null };
const loggedIn: SessionInfo = { authenticated: true, id: 'u1', username: 'ada', name: 'Ada' };

/** The bar's own links, excluding anything the drawer contributes -- the
 * drawer is closed in every case that uses this, but scoping to the
 * <nav> keeps that true even if that changes. */
function barLinkNames() {
  return within(screen.getByRole('navigation'))
    .getAllByRole('link')
    .map((link) => link.textContent);
}

describe('AppNavBar', () => {
  beforeEach(() => {
    pathname = '/';
  });

  it('renders its links inside a navigation landmark', () => {
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('navigation')).toBeInTheDocument();
  });

  it('links to every primary destination, for anyone', () => {
    for (const session of [loggedOut, loggedIn]) {
      const { unmount } = renderWithMantine(<AppNavBar session={session} freshness={freshness} />);
      for (const destination of PRIMARY_NAV_DESTINATIONS) {
        expect(screen.getByRole('link', { name: destination.label })).toHaveAttribute('href', destination.href);
      }
      unmount();
    }
  });

  it('has one Track entry, and no separate train search or tracked-trains link', () => {
    for (const session of [loggedOut, loggedIn]) {
      const { unmount } = renderWithMantine(<AppNavBar session={session} freshness={freshness} />);
      expect(screen.getByRole('link', { name: 'Track' })).toHaveAttribute('href', '/track/mine');
      expect(barLinkNames()).not.toContain('Trains');
      expect(barLinkNames()).not.toContain('Track a journey');
      expect(barLinkNames()).not.toContain('My trains & tickets');
      unmount();
    }
  });

  it.each(['/trains', '/journeys/new', '/track', '/track/mine'])('marks Track current on %s', (path) => {
    pathname = path;
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('link', { name: 'Track' })).toHaveAttribute('aria-current', 'page');
  });

  it('never advertises Groups in the bar itself', () => {
    // Logged out it has no useful landing state at all; logged in it
    // belongs to the account menu. Either way it is not a bar link.
    for (const session of [loggedOut, loggedIn]) {
      const { unmount } = renderWithMantine(<AppNavBar session={session} freshness={freshness} />);
      expect(barLinkNames()).not.toContain('Groups');
      unmount();
    }
  });

  it('shows a log in link, and no account menu, to an anonymous visitor', () => {
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('link', { name: 'Log in' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Account menu for/ })).not.toBeInTheDocument();
  });

  it('keeps the brand and the freshness readout in the bar, and no theme or pride control', () => {
    // Both settings live on /account/preferences now; the saved pride
    // flag is still applied (PrideModeApplier).
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('link', { name: 'Distant Signal' })).toHaveAttribute('href', '/');
    expect(screen.queryByRole('button', { name: 'Colour theme' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Pride/ })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Data freshness' })).toBeInTheDocument();
  });

  describe('the "Plan a journey" entry', () => {
    it('links to /plan for anyone, straight after Track, at every width from md up', () => {
      for (const session of [loggedOut, loggedIn]) {
        const { unmount } = renderWithMantine(<AppNavBar session={session} freshness={freshness} />);
        const names = barLinkNames();
        expect(names[names.indexOf('Track') + 1]).toBe('Plan a journey');
        const link = screen.getByRole('link', { name: 'Plan a journey' });
        expect(link).toHaveAttribute('href', '/plan');
        expect(link.closest('.mantine-visible-from-lg')).toBeNull();
        unmount();
      }
    });

    it('marks itself as the current page on /plan, and nothing else', () => {
      pathname = '/plan';
      renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
      expect(screen.getByRole('link', { name: 'Plan a journey' })).toHaveAttribute('aria-current', 'page');
      expect(screen.getByRole('link', { name: 'Track' })).not.toHaveAttribute('aria-current');
    });

    it('is in the drawer, marked current on /plan', async () => {
      pathname = '/plan';
      renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
      fireEvent.click(screen.getByRole('button', { name: 'Navigation menu' }));
      const drawer = await screen.findByRole('dialog');
      const link = within(drawer).getByRole('link', { name: PLAN_JOURNEY_DESTINATION.label });
      expect(link).toHaveAttribute('href', '/plan');
      expect(link).toHaveAttribute('aria-current', 'page');
      expect(link).toHaveAttribute('data-active', 'true');
    });
  });

  it('marks no bar link as current on a page the bar does not list', () => {
    pathname = '/';
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    const current = within(screen.getByRole('navigation'))
      .getAllByRole('link')
      .filter((link) => link.getAttribute('aria-current') === 'page');
    expect(current).toEqual([]);
  });

  describe('the drawer it hands the small-screen navigation to', () => {
    function openDrawer() {
      fireEvent.click(screen.getByRole('button', { name: 'Navigation menu' }));
    }

    it('carries every primary destination plus Preferences', async () => {
      renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
      openDrawer();
      const drawer = await screen.findByRole('dialog');

      for (const destination of [...PRIMARY_NAV_DESTINATIONS, PREFERENCES_DESTINATION]) {
        expect(within(drawer).getByRole('link', { name: destination.label })).toHaveAttribute('href', destination.href);
      }
    });

    it('adds Groups only for an authenticated visitor', async () => {
      renderWithMantine(<AppNavBar session={loggedIn} freshness={freshness} />);
      openDrawer();
      expect(within(await screen.findByRole('dialog')).getByRole('link', { name: 'Groups' })).toHaveAttribute(
        'href',
        '/groups',
      );
    });

    it('omits Groups for an anonymous visitor, who has nothing to see there', async () => {
      renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
      openDrawer();
      expect(within(await screen.findByRole('dialog')).queryByRole('link', { name: 'Groups' })).toBeNull();
    });

    // Review §3.1.3: /chat was undiscoverable -- reachable only by typing
    // the URL directly. The drawer is the only nav surface below `md`, so
    // it has to carry Chat too, not just the account menu.
    it('adds Chat for every logged-in visitor', async () => {
      renderWithMantine(<AppNavBar session={loggedIn} freshness={freshness} />);
      openDrawer();
      expect(within(await screen.findByRole('dialog')).getByRole('link', { name: 'Ask about trains' })).toHaveAttribute(
        'href',
        '/chat',
      );
    });

    it('omits Chat for an anonymous visitor', async () => {
      renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
      openDrawer();
      expect(within(await screen.findByRole('dialog')).queryByRole('link', { name: 'Ask about trains' })).toBeNull();
    });
  });
});

// I4 regression guard (2026-09-17 whole-branch review): commit 5fcc5686
// fixed the auth Suspense fallback's colour to match the resolved
// `<LoginControl>`'s (both render the literal string "Log in", and the
// boundary is identical on every route, so a mismatch here is purely a
// function of streaming timing, not anything route-specific) and added a
// regression test pinning the two equal -- that test lived in
// `app/layout.test.tsx` and was lost, without an equivalent replacement,
// when the nav bar was extracted into this file. The fallback itself also
// silently reverted to `c="dimmed"` in the same rewrite.
describe('auth Suspense fallback colour', () => {
  it("uses the same colour token LoginControl/TextLink resolve to ('anchor'), not a separate grey", () => {
    // Source-text comparison, not a rendered-DOM one: `AuthStatus` never
    // actually suspends in a test (it takes an already-resolved `session`
    // prop), so the only way to observe this Suspense boundary's fallback
    // at all is to read its literal JSX. Deliberately reads BOTH values
    // from source rather than hardcoding either as a literal in this test,
    // so the two call sites are compared to each other, not to a copy that
    // could quietly drift from one of them.
    const navBarSource = readFileSync('components/AppNavBar.tsx', 'utf8');
    const textLinkSource = readFileSync('components/TextLink.tsx', 'utf8');
    // Whitespace-tolerant so Prettier's line-wrapping of the JSX can't break it.
    const fallbackMatch = /<Suspense\s+fallback=\{\s*<Text size="sm" c="([^"]+)">\s*Log in\s*<\/Text>\s*\}\s*>/.exec(
      navBarSource,
    );
    const textLinkMatch = /const ANCHOR_COLOR = '([^']+)'/.exec(textLinkSource);
    expect(fallbackMatch).not.toBeNull();
    expect(textLinkMatch).not.toBeNull();
    expect(fallbackMatch?.[1]).toBe(textLinkMatch?.[1]);
  });
});
