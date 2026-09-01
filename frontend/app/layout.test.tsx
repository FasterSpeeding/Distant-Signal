import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackedTrainsNavItem, viewport, metadata } from './layout';
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

describe('metadata.appleWebApp', () => {
  it('sets only statusBarStyle to black-translucent -- no capable, no title', () => {
    // Exact-shape check, not just a `.statusBarStyle` field check: this is
    // the one place this plan's Global Constraints must hold structurally
    // -- `capable`/`title` must never be added alongside this.
    expect(metadata.appleWebApp).toEqual({ statusBarStyle: 'black-translucent' });
  });
});
