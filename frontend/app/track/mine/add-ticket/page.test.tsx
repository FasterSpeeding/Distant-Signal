import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AddTicketPage from './page';
import * as api from '@/lib/api';

vi.mock('@/lib/api');
// AutoOpenLoginPrompt -> LoginPromptModal calls useLoginHref()
// (usePathname()/useSearchParams() under the hood), and the expanded
// TicketEntryForm calls useRouter() -- same workaround
// app/track/mine/page.test.tsx and TicketEntryForm.test.tsx use for the
// same reason (both hooks throw outside a real Next.js App Router tree).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/track/mine/add-ticket',
  useSearchParams: () => new URLSearchParams(''),
}));

function session(authenticated: boolean) {
  return { authenticated, id: authenticated ? 'user-1' : null, email: null, name: null };
}

describe('AddTicketPage', () => {
  it('not logged in: shows an auto-opened login prompt modal, no form', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(false));
    renderWithMantine(await AddTicketPage());

    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText('Log in to add a ticket.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack%2Fmine%2Fadd-ticket',
    );
    expect(screen.queryByLabelText('Operator')).not.toBeInTheDocument();
  });

  it('logged in: shows the heading, a Back link, and TicketEntryForm expanded with no click needed', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    renderWithMantine(await AddTicketPage());

    expect(screen.getByRole('heading', { name: 'Add a ticket', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to My Trains & Tickets' })).toHaveAttribute(
      'href',
      '/track/mine',
    );
    // defaultOpen: the manual-entry fields are visible immediately, no
    // collapsed-button click required.
    expect(screen.getByLabelText('Operator')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a ticket' })).not.toBeInTheDocument();
  });

  it('the rendered TicketEntryForm has no trackingId: a save posts to the flat /api/Train/tickets route', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 })));
    renderWithMantine(await AddTicketPage());

    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets', expect.objectContaining({ method: 'POST' }));
    });
    vi.unstubAllGlobals();
  });
});
