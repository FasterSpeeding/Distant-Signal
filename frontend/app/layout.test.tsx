import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackedTrainsNavItem, viewport } from './layout';
import * as api from '@/lib/api';

vi.mock('@/lib/api');

// Part B of the upload-first ticket-tracking plan merged /track/mine and
// /track/tickets into one page -- this nav item is now the single entry
// point to both (the separate MyTicketsNavItem this file used to also
// export/test, pointing at the now-redirected /track/tickets, is gone).
describe('TrackedTrainsNavItem', () => {
  it('hides "My Trains & Tickets" when logged out', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    renderWithMantine(await TrackedTrainsNavItem());
    // Not `toBeEmptyDOMElement()` on the container: MantineProvider injects
    // <style> tags into the render tree regardless, so the container is
    // never literally empty (see TicketPanel.test.tsx's 404 case for the
    // same established workaround on other `return null` components).
    expect(screen.queryByRole('link', { name: 'My Trains & Tickets' })).not.toBeInTheDocument();
  });

  it('shows "My Trains & Tickets", pointing at /track/mine, when logged in', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: 'a@b.com', name: 'Ada' });
    renderWithMantine(await TrackedTrainsNavItem());
    expect(screen.getByRole('link', { name: 'My Trains & Tickets' })).toHaveAttribute('href', '/track/mine');
  });

  it('degrades to hidden (not a thrown error) when getSession rejects', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('auth service unreachable'));
    renderWithMantine(await TrackedTrainsNavItem());
    expect(screen.queryByRole('link', { name: 'My Trains & Tickets' })).not.toBeInTheDocument();
  });
});

describe('viewport', () => {
  it('defaults color-scheme to light for the pre-hydration SSR render', () => {
    // No route in this app defines its own viewport/metadata export
    // (confirmed by grep against frontend/app/ — this worktree's plan doc
    // for this feature cites the same check), so this root-level default
    // is the value Next actually renders for every page. 'light' matches
    // ThemeToggle's own pre-mount fallback (useComputedColorScheme('light')),
    // not a new, third opinion about what "unknown" means.
    expect(viewport.colorScheme).toBe('light');
  });
});
