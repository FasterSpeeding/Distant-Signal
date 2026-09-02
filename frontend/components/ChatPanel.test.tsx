import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ChatPanel } from './ChatPanel';

/** Builds a `Response` whose body streams the given SSE `data: ...`
 * payloads one chunk at a time -- mirrors what `frontend/app/api/chat/route.ts`
 * hands back verbatim from `orchestrator/`'s own `POST /chat` (Task 3's SSE
 * emission), without a live network call. */
function sseResponse(events: unknown[], status = 200): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream({
    start(controller) {
      for (const event of events) {
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`));
      }
      controller.close();
    },
  });
  return new Response(body, { status, headers: { 'Content-Type': 'text/event-stream' } });
}

describe('ChatPanel', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders a placeholder prompt before any message is sent', () => {
    renderWithMantine(<ChatPanel />);
    expect(screen.getByText(/Ask about live departures/)).toBeInTheDocument();
  });

  it('sends the typed message to /api/chat and renders the streamed reply', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        sseResponse([
          { type: 'text-delta', text: 'The next ' },
          { type: 'text-delta', text: 'train is at 10:32.' },
          { type: 'done' },
        ]),
      ),
    );
    renderWithMantine(<ChatPanel />);

    fireEvent.change(screen.getByPlaceholderText(/next train/), { target: { value: "when's the next train?" } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    expect(screen.getByText("when's the next train?")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('The next train is at 10:32.')).toBeInTheDocument());

    const [url, init] = vi.mocked(fetch).mock.calls[0];
    expect(url).toBe('/api/chat');
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({ message: "when's the next train?" });
  });

  it('clears the input immediately on submit', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([{ type: 'done' }])));
    renderWithMantine(<ChatPanel />);
    const input = screen.getByPlaceholderText(/next train/) as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'hello' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(input.value).toBe('');
    await waitFor(() => expect(fetch).toHaveBeenCalled());
  });

  it('renders a "track this leg" card with the correct href for a plan_journey tool-result event', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        sseResponse([
          { type: 'text-delta', text: 'Here is a journey option.' },
          {
            type: 'tool-result',
            toolName: 'plan_journey',
            structuredContent: {
              kind: 'train',
              from: { tiploc: 'KNGX', name: 'London Kings Cross', crs: 'KGX' },
              to: { tiploc: 'YORK', name: 'York', crs: 'YRK' },
              departure: '10:32',
              arrival: '12:01',
              departureAt: '2026-09-02T10:32:00Z',
              arrivalAt: '2026-09-02T12:01:00Z',
              operator: 'LNER',
              uid: 'C12345',
            },
          },
          { type: 'done' },
        ]),
      ),
    );
    renderWithMantine(<ChatPanel />);

    fireEvent.change(screen.getByPlaceholderText(/next train/), { target: { value: 'plan a journey to york' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => expect(screen.getByRole('link', { name: 'Track this train' })).toBeInTheDocument());
    const link = screen.getByRole('link', { name: 'Track this train' });
    expect(link).toHaveAttribute('href', '/track?origin=KGX');
  });

  it('does not render a track link for a leg with no CRS, but still renders the card text', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        sseResponse([
          {
            type: 'tool-result',
            toolName: 'plan_journey',
            structuredContent: {
              kind: 'train',
              from: { tiploc: 'KNGX', name: 'London Kings Cross', crs: null },
              to: { tiploc: 'YORK', name: 'York', crs: 'YRK' },
              departure: '10:32',
              arrival: '12:01',
              departureAt: null,
              arrivalAt: null,
              operator: null,
              uid: 'C12345',
            },
          },
          { type: 'done' },
        ]),
      ),
    );
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/next train/), { target: { value: 'plan a journey' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => expect(screen.getByText(/London Kings Cross → York/)).toBeInTheDocument());
    expect(screen.queryByRole('link', { name: 'Track this train' })).not.toBeInTheDocument();
  });

  it('shows an error and removes the pending turn on a 403 response', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: 'chatbot_not_available' }), { status: 403 })));
    renderWithMantine(<ChatPanel />);
    fireEvent.change(screen.getByPlaceholderText(/next train/), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => expect(screen.getByText(/not available for your account/)).toBeInTheDocument());
  });

  it('does not submit an empty or whitespace-only message', () => {
    vi.stubGlobal('fetch', vi.fn());
    renderWithMantine(<ChatPanel />);
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText(/next train/), { target: { value: '   ' } });
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    expect(fetch).not.toHaveBeenCalled();
  });
});
