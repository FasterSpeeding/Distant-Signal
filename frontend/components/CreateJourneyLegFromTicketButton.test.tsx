import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CreateJourneyLegFromTicketButton } from './CreateJourneyLegFromTicketButton';
import type { JourneyLegProposal } from '@/lib/types';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

function proposal(overrides: Partial<JourneyLegProposal> = {}): JourneyLegProposal {
  return {
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    serviceDate: '2026-09-22',
    departAfter: '19:02:00',
    departBefore: '20:02:00',
    ...overrides,
  };
}

describe('CreateJourneyLegFromTicketButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fetches the proposal and navigates to /track pre-filled in window mode, never creating anything itself', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(JSON.stringify(proposal()), { status: 200 }));
    renderWithMantine(<CreateJourneyLegFromTicketButton ticketId={5} />);

    fireEvent.click(screen.getByRole('button', { name: 'Create a journey leg from this ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/5/journey-leg-proposal');
    });
    // Only ever a GET against the read-only proposal endpoint -- no POST,
    // to `/api/Journeys` or anywhere else, from this component at all.
    expect(fetch).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith(
        '/track?mode=window&origin=KGX&destination=EDB&serviceDate=2026-09-22&departAfter=19%3A02&departBefore=20%3A02',
      );
    });
  });

  it('omits query params the proposal did not know, rather than sending empty values', async () => {
    vi.mocked(fetch).mockResolvedValue(
      new Response(
        JSON.stringify(
          proposal({ serviceDate: null, departAfter: null, departBefore: null }),
        ),
        { status: 200 },
      ),
    );
    renderWithMantine(<CreateJourneyLegFromTicketButton ticketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Create a journey leg from this ticket' }));

    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/track?mode=window&origin=KGX&destination=EDB');
    });
  });

  it('on a 401, shows a login-specific message and never navigates', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    renderWithMantine(<CreateJourneyLegFromTicketButton ticketId={5} />);

    fireEvent.click(screen.getByRole('button', { name: 'Create a journey leg from this ticket' }));

    expect(await screen.findByText('Log in to create a journey leg from this ticket.')).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('on any other failure status, shows a generic error and never navigates', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no ticket with that id', { status: 404 }));
    renderWithMantine(<CreateJourneyLegFromTicketButton ticketId={5} />);

    fireEvent.click(screen.getByRole('button', { name: 'Create a journey leg from this ticket' }));

    expect(await screen.findByText("Couldn't load a proposal for this ticket.")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('on a network failure, shows a generic error and never navigates', async () => {
    vi.mocked(fetch).mockRejectedValue(new Error('network down'));
    renderWithMantine(<CreateJourneyLegFromTicketButton ticketId={5} />);

    fireEvent.click(screen.getByRole('button', { name: 'Create a journey leg from this ticket' }));

    expect(await screen.findByText("Couldn't load a proposal for this ticket.")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });
});
