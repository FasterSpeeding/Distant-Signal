import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackedTrainsNavItem } from './layout';
import * as api from '@/lib/api';

vi.mock('@/lib/api');

describe('TrackedTrainsNavItem', () => {
  it('hides "My Tracked Trains" when logged out', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    renderWithMantine(await TrackedTrainsNavItem());
    // Not `toBeEmptyDOMElement()` on the container: MantineProvider injects
    // <style> tags into the render tree regardless, so the container is
    // never literally empty (see TicketPanel.test.tsx's 404 case for the
    // same established workaround on other `return null` components).
    expect(screen.queryByRole('link', { name: 'My Tracked Trains' })).not.toBeInTheDocument();
  });

  it('shows "My Tracked Trains", pointing at /track/mine, when logged in', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: 'a@b.com', name: 'Ada' });
    renderWithMantine(await TrackedTrainsNavItem());
    expect(screen.getByRole('link', { name: 'My Tracked Trains' })).toHaveAttribute('href', '/track/mine');
  });
});

// AuthNavItem/DataFreshnessNavItem stay local/unexported, matching their
// existing convention -- but rendering through the default RootLayout
// export isn't a viable way to test MyTicketsNavItem in isolation
// (RootLayout is synchronous and doesn't await its own Suspense children
// in a unit-render, so a mocked getSession() rejection/resolution can't be
// observed that way). Simplest robust option: export `MyTicketsNavItem`
// from layout.tsx (adding `export` to its function declaration, Step 2
// above) purely for this test's `import` statement to reach it directly --
// mirroring how this plan's other async Server Component tests (Task 6,
// TicketPanel.test.tsx) call their exported target directly rather than
// rendering through a parent.
import { MyTicketsNavItem } from './layout';

describe('MyTicketsNavItem', () => {
  it('hides "My Tickets" when logged out', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    renderWithMantine(await MyTicketsNavItem());
    expect(screen.queryByRole('link', { name: 'My Tickets' })).not.toBeInTheDocument();
  });

  it('shows "My Tickets", pointing at /track/tickets, when logged in', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: null });
    renderWithMantine(await MyTicketsNavItem());
    expect(screen.getByRole('link', { name: 'My Tickets' })).toHaveAttribute('href', '/track/tickets');
  });

  it('degrades to hidden (not a thrown error) when getSession rejects', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('auth service unreachable'));
    renderWithMantine(await MyTicketsNavItem());
    expect(screen.queryByRole('link', { name: 'My Tickets' })).not.toBeInTheDocument();
  });
});
