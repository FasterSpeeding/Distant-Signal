import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneySuggestionsUnmute } from './JourneySuggestionsUnmute';

const refresh = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh }),
}));

afterEach(() => {
  vi.unstubAllGlobals();
  refresh.mockReset();
});

describe('JourneySuggestionsUnmute', () => {
  it('turns the journey suggestions back on and reloads the page', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<JourneySuggestionsUnmute journeyId={12} />);
    expect(screen.getByText('Suggestion notifications are off for this journey.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Turn suggestions back on' }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/12/suggestions-muted', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ muted: false }),
    });
  });

  it('says so when it fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 500 })),
    );
    renderWithMantine(<JourneySuggestionsUnmute journeyId={12} />);
    fireEvent.click(screen.getByRole('button', { name: 'Turn suggestions back on' }));
    expect(await screen.findByText("Couldn't turn on suggestions. Try again.")).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();
  });
});
