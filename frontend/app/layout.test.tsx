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
