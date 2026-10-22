import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneySuggestionsToggle } from './JourneySuggestionsToggle';

// The switch's label wraps its description too, so the name starts with it.
const LABEL = /^Suggest other trains when a journey breaks/;

function stubFetch(put: () => Response) {
  const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) =>
    init?.method === 'PUT' ? put() : new Response(JSON.stringify({ journeySuggestions: true }), { status: 200 }),
  );
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('JourneySuggestionsToggle', () => {
  it('reads the saved value and is disabled until then', async () => {
    stubFetch(() => new Response(null, { status: 204 }));
    renderWithMantine(<JourneySuggestionsToggle />);
    const toggle = screen.getByRole('switch', { name: LABEL });
    expect(toggle).toBeDisabled();
    await waitFor(() => expect(toggle).toBeChecked());
    expect(toggle).toBeEnabled();
  });

  it('saves a change', async () => {
    const fetchMock = stubFetch(() => new Response(null, { status: 204 }));
    renderWithMantine(<JourneySuggestionsToggle />);
    const toggle = screen.getByRole('switch', { name: LABEL });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(fetchMock).toHaveBeenCalledWith('/api/preferences/notifications', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ journeySuggestions: false }),
    });
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('puts the switch back and says so when the save fails', async () => {
    stubFetch(() => new Response('', { status: 500 }));
    renderWithMantine(<JourneySuggestionsToggle />);
    const toggle = screen.getByRole('switch', { name: LABEL });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(await screen.findByText("Couldn't save this setting. Try again.")).toBeInTheDocument();
    expect(toggle).toBeChecked();
  });

  it('says when it could not load the setting', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 500 })),
    );
    renderWithMantine(<JourneySuggestionsToggle />);
    expect(await screen.findByText("Couldn't load this setting. Try again.")).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: LABEL })).toBeDisabled();
  });
});
