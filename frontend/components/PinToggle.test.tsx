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
    expect(screen.getByLabelText('Unpin (currently pinned)')).toBeInTheDocument();
  });

  it('states both the action and the current state in its accessible name', () => {
    // Two separate instances rather than rerendering one: `pinned` is
    // seeded from `initiallyPinned` via `useState` and, by design, doesn't
    // resync to a changed prop on rerender of the same component instance.
    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    expect(screen.getByLabelText('Pin (currently not pinned)')).toBeInTheDocument();

    renderWithProvider(<PinToggle kind="line" id="ecml" initiallyPinned={true} />);
    expect(screen.getByLabelText('Unpin (currently pinned)')).toBeInTheDocument();
  });

  it('renders a tooltip for sighted users with the same text as the accessible name', async () => {
    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.mouseEnter(screen.getByLabelText('Pin (currently not pinned)'));
    // `hidden` isn't a valid option for `findByText` (it's `getByRole`-only,
    // per `@testing-library/dom`'s `SelectorMatcherOptions` — no `hidden`
    // field); `selector` alone is what actually scopes this to the tooltip.
    expect(await screen.findByText('Pin (currently not pinned)', { selector: '[role="tooltip"]' })).toBeInTheDocument();
  });

  it('distinguishes pinned from unpinned by more than icon fill alone (color also differs)', () => {
    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    const unpinnedButton = screen.getByLabelText('Pin (currently not pinned)');
    const unpinnedStyle = unpinnedButton.getAttribute('style');

    renderWithProvider(<PinToggle kind="line" id="ecml" initiallyPinned={true} />);
    const pinnedButton = screen.getByLabelText('Unpin (currently pinned)');
    const pinnedStyle = pinnedButton.getAttribute('style');

    // Mantine resolves `color` into `--ai-bg`/`--ai-color` CSS vars on the
    // root element's inline style, so a genuinely different color (not just
    // a different `variant`) shows up as a different style attribute here.
    expect(pinnedStyle).not.toBe(unpinnedStyle);
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
    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

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
    fireEvent.click(screen.getByLabelText('Unpin (currently pinned)'));

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
