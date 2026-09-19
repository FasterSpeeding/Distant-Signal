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
    expect(screen.queryByRole('combobox', { name: 'Operator (optional)' })).not.toBeInTheDocument();
  });

  it('not logged in: also renders a server-rendered LoginLink, not just the client-only modal', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(false));
    renderWithMantine(await AddTicketPage());

    const link = screen.getByRole('link', { name: 'Log in to add a ticket' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrack%2Fmine%2Fadd-ticket');
  });

  it('logged in: shows the heading, the standalone-ticket explainer sentence, a Back link, and TicketEntryForm expanded with no click needed', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    renderWithMantine(await AddTicketPage());

    expect(screen.getByRole('heading', { name: 'Add a ticket', level: 1 })).toBeInTheDocument();
    // Task 3.6.5: this page never said which train the ticket attaches to,
    // or that it doesn't need one yet.
    expect(
      screen.getByText(
        "Save the ticket now; you can attach it to a tracked train afterwards, or we'll try to match it for you.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to My Trains & Tickets' })).toHaveAttribute(
      'href',
      '/track/mine',
    );
    // defaultOpen: the manual-entry fields are visible immediately, no
    // collapsed-button click required.
    expect(screen.getByRole('combobox', { name: 'Operator (optional)' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a ticket' })).not.toBeInTheDocument();
  });

  it('the rendered TicketEntryForm has no trackingId: a save posts to the flat /api/Train/tickets route', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    // Routes the Operator field's own debounced `/api/tocs?q=` suggestion
    // fetch (`TicketEntryForm.tsx`'s Task 3.6.4 Autocomplete) away from the
    // single mocked ticket-save response -- same hazard, same fix, as
    // `TicketEntryForm.test.tsx`'s own `mockDefaultResponse` helper.
    vi.stubGlobal(
      'fetch',
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (url.startsWith('/api/stations?') || url.startsWith('/api/tocs?')) {
          return Promise.resolve(new Response('[]', { status: 200 }));
        }
        return Promise.resolve(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 }));
      }),
    );
    renderWithMantine(await AddTicketPage());

    fireEvent.change(screen.getByRole('combobox', { name: 'Operator (optional)' }), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets', expect.objectContaining({ method: 'POST' }));
    });
    vi.unstubAllGlobals();
  });
});
