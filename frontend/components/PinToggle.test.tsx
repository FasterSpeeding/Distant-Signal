import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { PinToggle } from './PinToggle';

// PinToggle calls useRouter() from next/navigation, which throws
// "invariant expected app router to be mounted" when rendered outside a
// real Next.js App Router tree (as in these unit tests). Stub it so the
// component can render; router.refresh() itself isn't under test here.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
}));

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe('PinToggle', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows a filled star when initially pinned, outline when not', () => {
    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={true} />);
    expect(screen.getByLabelText('Unpin')).toBeInTheDocument();
  });

  it('pinning fetches current preferences then PUTs the id appended', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: ['swr-alton'], pinnedStations: [] }), { status: 200 });
      }
      return new Response(null, { status: 204 });
    });

    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/preferences/pinned-lines',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify(['swr-alton', 'wcml']),
        }),
      );
    });
  });

  it('unpinning fetches current preferences then PUTs the id removed', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: [], pinnedStations: ['WOK', 'AON'] }), { status: 200 });
      }
      return new Response(null, { status: 204 });
    });

    renderWithProvider(<PinToggle kind="station" id="WOK" initiallyPinned={true} />);
    fireEvent.click(screen.getByLabelText('Unpin'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/preferences/pinned-stations',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify(['AON']),
        }),
      );
    });
  });
});
