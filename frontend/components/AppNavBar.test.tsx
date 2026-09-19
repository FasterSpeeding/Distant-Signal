import { readFileSync } from 'node:fs';
import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AppNavBar } from './AppNavBar';
import { PRIMARY_NAV_DESTINATIONS } from '@/lib/navLinks';
import type { DataFreshness, SessionInfo } from '@/lib/types';

// LoginLink / AccountMenu / AppNavDrawer all reach for App Router hooks
// that throw outside a real Next.js tree -- same stub as the rest of this
// directory's tests.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

const freshness: DataFreshness = {
  stations: null,
  tocs: null,
  incidents: null,
  tfl: null,
  schedule_feed: null,
};

const loggedOut: SessionInfo = { authenticated: false, id: null, email: null, name: null };
const loggedIn: SessionInfo = { authenticated: true, id: 'u1', email: 'ada@example.com', name: 'Ada' };

/** The bar's own links, excluding anything the drawer contributes -- the
 * drawer is closed in every case that uses this, but scoping to the
 * <nav> keeps that true even if that changes. */
function barLinkNames() {
  return within(screen.getByRole('navigation'))
    .getAllByRole('link')
    .map((link) => link.textContent);
}

describe('AppNavBar', () => {
  it('renders its links inside a navigation landmark', () => {
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('navigation')).toBeInTheDocument();
  });

  it('links to every primary destination, for anyone', () => {
    for (const session of [loggedOut, loggedIn]) {
      const { unmount } = renderWithMantine(<AppNavBar session={session} freshness={freshness} />);
      for (const destination of PRIMARY_NAV_DESTINATIONS) {
        expect(screen.getByRole('link', { name: destination.label })).toHaveAttribute(
          'href',
          destination.href,
        );
      }
      unmount();
    }
  });

  /** Regression guard carried over from the old source-level assertion in
   * app/layout.test.tsx: /trains is an ADDITION, not a replacement. The
   * design doc's §4 is an explicit "no" on removing or hiding /track, and
   * the two station/line entry points either side of the newer link must
   * survive it. */
  it('keeps the lines, stations and train-search entry points together', () => {
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('link', { name: 'All Lines' })).toHaveAttribute('href', '/lines');
    expect(screen.getByRole('link', { name: 'Station Lookup' })).toHaveAttribute('href', '/stations');
    expect(screen.getByRole('link', { name: 'Find a Train' })).toHaveAttribute('href', '/trains');
  });

  it('offers "My Trains & Tickets" inline to an anonymous visitor, who has no account menu to find it in', () => {
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('link', { name: 'My Trains & Tickets' })).toHaveAttribute(
      'href',
      '/track/mine',
    );
  });

  it('moves "My Trains & Tickets" out of the bar once there is an account menu to hold it', () => {
    // This, plus the display name and the "Log out" button collapsing
    // into one avatar, is what stops the authenticated bar wrapping onto
    // a second row at 1440px. The destination itself is still reachable
    // -- from the account menu (AccountMenu.test.tsx) and from the drawer
    // (below).
    renderWithMantine(<AppNavBar session={loggedIn} freshness={freshness} />);
    expect(barLinkNames()).not.toContain('My Trains & Tickets');
    expect(screen.getByRole('button', { name: 'Account menu for Ada' })).toBeInTheDocument();
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

  it('keeps the brand, the theme toggle, the pride toggle and the freshness readout in the bar', () => {
    // The four things the mobile collapse must NOT sweep into the drawer.
    renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
    expect(screen.getByRole('link', { name: 'Distant Signal' })).toHaveAttribute('href', '/');
    expect(screen.getByRole('button', { name: /^Theme:/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Pride/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Data freshness' })).toBeInTheDocument();
  });

  describe('the drawer it hands the small-screen navigation to', () => {
    function openDrawer() {
      fireEvent.click(screen.getByRole('button', { name: 'Navigation menu' }));
    }

    it('carries every primary destination plus "My Trains & Tickets"', async () => {
      renderWithMantine(<AppNavBar session={loggedOut} freshness={freshness} />);
      openDrawer();
      const drawer = await screen.findByRole('dialog');

      for (const destination of [...PRIMARY_NAV_DESTINATIONS, { label: 'My Trains & Tickets', href: '/track/mine' }]) {
        expect(within(drawer).getByRole('link', { name: destination.label })).toHaveAttribute(
          'href',
          destination.href,
        );
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
    it('adds Chat only when chatAllowed is true', async () => {
      renderWithMantine(<AppNavBar session={loggedIn} freshness={freshness} chatAllowed />);
      openDrawer();
      expect(within(await screen.findByRole('dialog')).getByRole('link', { name: 'Chat' })).toHaveAttribute(
        'href',
        '/chat',
      );
    });

    it('omits Chat by default (chatAllowed not yet confirmed)', async () => {
      renderWithMantine(<AppNavBar session={loggedIn} freshness={freshness} />);
      openDrawer();
      expect(within(await screen.findByRole('dialog')).queryByRole('link', { name: 'Chat' })).toBeNull();
    });
  });
});

// I4 regression guard (2026-09-17 whole-branch review): commit 5fcc5686
// fixed the auth Suspense fallback's colour to match the resolved
// `<LoginLink>`'s (both render the literal string "Log in", and the
// boundary is identical on every route, so a mismatch here is purely a
// function of streaming timing, not anything route-specific) and added a
// regression test pinning the two equal -- that test lived in
// `app/layout.test.tsx` and was lost, without an equivalent replacement,
// when the nav bar was extracted into this file. The fallback itself also
// silently reverted to `c="dimmed"` in the same rewrite.
describe('auth Suspense fallback colour', () => {
  it("uses the same colour token LoginLink/TextLink resolve to ('anchor'), not a separate grey", () => {
    // Source-text comparison, not a rendered-DOM one: `AuthStatus` never
    // actually suspends in a test (it takes an already-resolved `session`
    // prop), so the only way to observe this Suspense boundary's fallback
    // at all is to read its literal JSX. Deliberately reads BOTH values
    // from source rather than hardcoding either as a literal in this test,
    // so the two call sites are compared to each other, not to a copy that
    // could quietly drift from one of them.
    const navBarSource = readFileSync('components/AppNavBar.tsx', 'utf8');
    const textLinkSource = readFileSync('components/TextLink.tsx', 'utf8');
    const fallbackMatch = navBarSource.match(/<Suspense fallback=\{<Text size="sm" c="([^"]+)">Log in<\/Text>\}>/);
    const textLinkMatch = textLinkSource.match(/<Text c="([^"]+)"/);
    expect(fallbackMatch).not.toBeNull();
    expect(textLinkMatch).not.toBeNull();
    expect(fallbackMatch?.[1]).toBe(textLinkMatch?.[1]);
  });
});
