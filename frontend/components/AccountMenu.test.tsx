import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AccountMenu } from './AccountMenu';
import type { NavDestination } from '@/lib/navLinks';

// `useLogout` calls useRouter() from next/navigation, which throws
// "invariant expected app router to be mounted" outside a real Next.js
// App Router tree — same stub AuthStatus.test.tsx and PinToggle.test.tsx
// use.
const refresh = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh }),
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

const destinations: NavDestination[] = [
  { href: '/track/mine', label: 'My Trains & Tickets' },
  { href: '/groups', label: 'Groups' },
];

function openMenu() {
  fireEvent.click(screen.getByRole('button', { name: 'Account menu for Ada' }));
}

/** `hidden: true` on every dropdown query, and it is a jsdom artefact
 * rather than a claim about the real page: Mantine's `Popover` (which
 * `Menu` is built on) applies floating-ui's `hide()` middleware, and in
 * jsdom every element measures 0x0, so the middleware concludes the
 * target is off-screen and sets `display: none` on the dropdown a tick
 * after it opens. Same reason the combobox option queries in
 * app/stations/StationSearchForm.test.tsx and
 * app/lines/CustomLineForm.test.tsx pass it. The real hidden-vs-shown
 * assertion is the open/closed one (`menuIsOpen` below), which is
 * unaffected because Mantine unmounts the dropdown entirely while
 * closed. */
function menuItem(name: string) {
  return screen.findByRole('menuitem', { name, hidden: true });
}

function menuIsOpen() {
  return screen.queryByRole('menu', { hidden: true }) !== null;
}

describe('AccountMenu', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refresh.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('names its target button after the visitor, so the display name survives leaving the bar', () => {
    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    expect(screen.getByRole('button', { name: 'Account menu for Ada' })).toBeInTheDocument();
  });

  it('shows the visitor initials in the bar', () => {
    // The sighted counterpart to the aria-label above: Mantine's Avatar
    // renders initials from `name` when it has no `src`.
    renderWithMantine(<AccountMenu label="Ada Lovelace" destinations={destinations} />);
    expect(screen.getByText('AL')).toBeInTheDocument();
  });

  it('keeps its destinations out of the bar until it is opened', () => {
    // Not merely hidden: Mantine unmounts the dropdown while closed, so
    // these are out of the tab order and out of a screen reader's
    // element list too.
    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    expect(menuIsOpen()).toBe(false);
    expect(screen.queryByRole('menuitem', { name: 'Groups', hidden: true })).not.toBeInTheDocument();
    expect(screen.queryByRole('menuitem', { name: 'Log out', hidden: true })).not.toBeInTheDocument();
  });

  it('renders every destination it is given as a real link', async () => {
    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();

    for (const destination of destinations) {
      expect(await menuItem(destination.label)).toHaveAttribute('href', destination.href);
    }
  });

  it('repeats the full display name inside the open menu', async () => {
    // The initials in the bar are an abbreviation; opening the menu is
    // where a visitor confirms WHICH account they are signed in as.
    renderWithMantine(<AccountMenu label="ada@example.com" destinations={destinations} />);
    fireEvent.click(screen.getByRole('button', { name: 'Account menu for ada@example.com' }));
    expect(await screen.findByText('ada@example.com')).toBeInTheDocument();
  });

  it('offers Log out as an action rather than a link', async () => {
    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();
    const logout = await menuItem('Log out');
    expect(logout).not.toHaveAttribute('href');
  });

  it('logging out posts to /api/auth/logout and refreshes the router', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();
    fireEvent.click(await menuItem('Log out'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/auth/logout', { method: 'POST' });
    });
    // The refresh is what re-runs the layout's server-side session check
    // and swaps this control back to the anonymous "Log in" link.
    await waitFor(() => {
      expect(refresh).toHaveBeenCalled();
    });
  });

  it('logging out other sessions posts to /api/auth/sessions/revoke-others and refreshes the router', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();
    fireEvent.click(await menuItem('Log out other sessions'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/auth/sessions/revoke-others', { method: 'POST' });
    });
    // The reissued session cookie means the router refresh should still
    // show this visitor logged in -- unlike `logout`, this does not log
    // the current browser out.
    await waitFor(() => {
      expect(refresh).toHaveBeenCalled();
    });
  });

  it('surfaces an error instead of refreshing when logging out other sessions fails', async () => {
    // Deliberately NOT the same "swallow and refresh anyway" behaviour
    // `useLogout` has for a failed logout -- a failed revoke-others
    // request means nothing was actually revoked, which is worth telling
    // the visitor rather than silently pretending it worked.
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 500 }));

    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();
    fireEvent.click(await menuItem('Log out other sessions'));

    expect(await menuItem('Could not log out other sessions -- try again')).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();
  });

  it('still refreshes when the logout request fails', async () => {
    // `/auth/logout` is idempotent and the cookie is gone either way, so
    // a failed request must not leave the nav showing a stale signed-in
    // state -- `useLogout`'s `finally` is what guarantees that.
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockRejectedValue(new Error('offline'));

    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();
    fireEvent.click(await menuItem('Log out'));

    await waitFor(() => {
      expect(refresh).toHaveBeenCalled();
    });
  });

  it('allows Tab to reach its items, as a navigation menu must', async () => {
    // Mantine's default `menuItemTabIndex` is -1, which is right for a
    // menu of actions and wrong for one that is mostly links.
    renderWithMantine(<AccountMenu label="Ada" destinations={destinations} />);
    openMenu();
    const item = await menuItem('Groups');
    expect(item).toHaveAttribute('tabindex', '0');
  });
});
