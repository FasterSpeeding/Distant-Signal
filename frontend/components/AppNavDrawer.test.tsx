import { useReducer } from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AppNavDrawer } from './AppNavDrawer';
import type { NavDestination } from '@/lib/navLinks';

// `usePathname()` throws outside a real Next.js App Router tree. A
// mutable stub (rather than a fixed string) so the close-on-navigate case
// below can actually change the pathname between renders — the same
// `vi.mock` shape AuthStatus.test.tsx uses, with one extra degree of
// freedom.
let pathname = '/';
vi.mock('next/navigation', () => ({
  usePathname: () => pathname,
  useRouter: () => ({ refresh: vi.fn() }),
  useSearchParams: () => new URLSearchParams(''),
}));

const destinations: NavDestination[] = [
  { href: '/lines', label: 'All Lines' },
  { href: '/stations', label: 'Station Lookup' },
  { href: '/track/mine', label: 'My Trains & Tickets' },
];

function burger() {
  return screen.getByRole('button', { name: 'Navigation menu' });
}

describe('AppNavDrawer', () => {
  beforeEach(() => {
    pathname = '/';
  });

  it('renders a burger that is collapsed to begin with', () => {
    renderWithMantine(<AppNavDrawer destinations={destinations} />);
    expect(burger()).toHaveAttribute('aria-expanded', 'false');
  });

  it('keeps every destination out of the DOM until the burger is pressed', () => {
    // Not merely hidden: Mantine's Drawer unmounts its contents while
    // closed, which is what keeps these links out of the tab order and
    // out of a screen reader's link list on a page they are not offered
    // on yet.
    renderWithMantine(<AppNavDrawer destinations={destinations} />);
    expect(screen.queryByRole('link', { name: 'All Lines' })).not.toBeInTheDocument();
  });

  it('opens a drawer listing every destination it is given, as real links', async () => {
    renderWithMantine(<AppNavDrawer destinations={destinations} />);
    fireEvent.click(burger());

    for (const destination of destinations) {
      const link = await screen.findByRole('link', { name: destination.label });
      expect(link).toHaveAttribute('href', destination.href);
    }
    expect(burger()).toHaveAttribute('aria-expanded', 'true');
  });

  it('names the drawer so its dialog is not anonymous to assistive tech', async () => {
    renderWithMantine(<AppNavDrawer destinations={destinations} title="Menu" />);
    fireEvent.click(burger());
    expect(await screen.findByRole('dialog', { name: 'Menu' })).toBeInTheDocument();
  });

  /** Mantine's `Drawer` close button ships with NO accessible name of its
   * own -- axe scored it `button-name`, CRITICAL, the first time the
   * e2e sweep opened this drawer. The fix is a `Drawer.defaultProps` in
   * `lib/theme.ts` (alongside the identical `Modal` one that predates it),
   * which is why this asserts through `renderWithMantine` -- that helper
   * supplies the real production theme, so a test passing here means the
   * app's own users get the name too. */
  it('gives the drawer close button an accessible name, via the app theme', async () => {
    renderWithMantine(<AppNavDrawer destinations={destinations} />);
    fireEvent.click(burger());
    await screen.findByRole('dialog');
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
  });

  /** Deliberately NOT a second `<nav>`/`role="navigation"`: the drawer is
   * portalled to document.body, outside the bar's own nav landmark, and a
   * second unnamed navigation landmark is what axe's `landmark-unique`
   * rule fires on. The dialog role plus its title is the accessible
   * structure here. */
  it('does not introduce a second navigation landmark', async () => {
    renderWithMantine(<AppNavDrawer destinations={destinations} />);
    fireEvent.click(burger());
    await screen.findByRole('dialog');
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument();
  });

  it('marks the current page among the destinations', async () => {
    pathname = '/stations';
    renderWithMantine(<AppNavDrawer destinations={destinations} />);
    fireEvent.click(burger());

    const current = await screen.findByRole('link', { name: 'Station Lookup' });
    expect(current).toHaveAttribute('data-active', 'true');
    expect(screen.getByRole('link', { name: 'All Lines' })).not.toHaveAttribute('data-active');
  });

  it('closes itself once a navigation has happened', async () => {
    // A soft navigation does not unmount the root layout, so without the
    // pathname effect the drawer would sit open on top of the page the
    // visitor just asked for.
    //
    // Re-rendered through a harness rather than @testing-library's own
    // `rerender`: that helper re-renders the bare element, which would
    // drop `renderWithMantine`'s MantineProvider wrapper and blow up on
    // the `useMantineTheme()` call inside the component. This forces a
    // re-render from INSIDE the provider instead, which is also a closer
    // model of what a real soft navigation does to this subtree.
    let navigate = () => {};
    function Harness() {
      const [, forceRender] = useReducer((tick: number) => tick + 1, 0);
      navigate = forceRender;
      return <AppNavDrawer destinations={destinations} />;
    }

    renderWithMantine(<Harness />);
    fireEvent.click(burger());
    await screen.findByRole('dialog');

    pathname = '/stations';
    act(() => navigate());

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });

  it('renders nothing at all for an empty destination list', () => {
    // Guards the reusable contract rather than this app's use of it: a
    // caller that computes an empty list should not get a burger that
    // opens onto nothing.
    renderWithMantine(<AppNavDrawer destinations={[]} />);
    fireEvent.click(burger());
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
});
