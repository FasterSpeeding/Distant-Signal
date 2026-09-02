import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PinToggle } from './PinToggle';

// PinToggle calls useRouter() from next/navigation, which throws
// "invariant expected app router to be mounted" when rendered outside a
// real Next.js App Router tree (as in these unit tests). Stub it so the
// component can render; router.refresh() itself isn't under test here.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/lines',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('PinToggle', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows a filled star when initially pinned, outline when not', () => {
    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={true} />);
    expect(screen.getByLabelText('Unpin (currently pinned)')).toBeInTheDocument();
  });

  it('states both the action and the current state in its accessible name', () => {
    // Two separate instances rather than rerendering one: `pinned` is
    // seeded from `initiallyPinned` via `useState` and, by design, doesn't
    // resync to a changed prop on rerender of the same component instance.
    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    expect(screen.getByLabelText('Pin (currently not pinned)')).toBeInTheDocument();

    renderWithMantine(<PinToggle kind="line" id="ecml" initiallyPinned={true} />);
    expect(screen.getByLabelText('Unpin (currently pinned)')).toBeInTheDocument();
  });

  it('renders a tooltip for sighted users with the same text as the accessible name', async () => {
    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.mouseEnter(screen.getByLabelText('Pin (currently not pinned)'));
    // `hidden` isn't a valid option for `findByText` (it's `getByRole`-only,
    // per `@testing-library/dom`'s `SelectorMatcherOptions` — no `hidden`
    // field); `selector` alone is what actually scopes this to the tooltip.
    expect(await screen.findByText('Pin (currently not pinned)', { selector: '[role="tooltip"]' })).toBeInTheDocument();
  });

  it('distinguishes pinned from unpinned by more than icon fill alone (color also differs)', () => {
    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    const unpinnedButton = screen.getByLabelText('Pin (currently not pinned)');
    const unpinnedStyle = unpinnedButton.getAttribute('style');

    renderWithMantine(<PinToggle kind="line" id="ecml" initiallyPinned={true} />);
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

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
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

    renderWithMantine(<PinToggle kind="station" id="WOK" initiallyPinned={true} />);
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

  // `/public/preferences` requires an authenticated user, so an anonymous
  // visitor gets a 401 whose body is not JSON. Parsing it unguarded threw
  // inside a `try` with no `catch`, which left the button permanently
  // disabled (the `finally` did re-enable it, but the rejection escaped as
  // an unhandled promise rejection and nothing else happened).
  it('a 401 from the preferences read leaves the button usable and issues no PUT', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async () => new Response('no session', { status: 401 }));

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    await waitFor(() => {
      expect(screen.getByLabelText('Pin (currently not pinned)')).not.toBeDisabled();
    });
    // Still unpinned, and no write was attempted.
    expect(screen.getByLabelText('Pin (currently not pinned)')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith('/api/preferences');
  });

  // Same scenario as above, but asserting the fix itself: a 401 must leave
  // some visible trace instead of the dead-click silence the comment on
  // `toggle()` describes, so an anonymous visitor can discover *why*
  // nothing happened and how to fix it.
  it('a 401 from the preferences read shows the login prompt modal, linking to /api/auth/login', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async () => new Response('no session', { status: 401 }));

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    expect(screen.queryByText('Log in to pin this line.')).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    expect(await screen.findByText('Log in to pin this line.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines',
    );
  });

  // A 401 on the PUT (read succeeded, write didn't) must surface the same
  // prompt — the anonymous-visitor case can fail at either step.
  it('a 401 from the PUT also shows the login prompt modal', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: [], pinnedStations: [] }), { status: 200 });
      }
      return new Response('no session', { status: 401 });
    });

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    expect(await screen.findByText('Log in to pin this line.')).toBeInTheDocument();
  });

  // A non-401 failure (e.g. a 500) keeps the old silent-bail behaviour —
  // only 401 is documented as "you need to log in".
  it('a non-401 failure does not show the login prompt', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async () => new Response('server error', { status: 500 }));

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    await waitFor(() => {
      expect(screen.getByLabelText('Pin (currently not pinned)')).not.toBeDisabled();
    });
    expect(screen.queryByText('Log in to pin this line.')).not.toBeInTheDocument();
  });

  // Same reasoning one step later in the flow: the read succeeded but the
  // write was rejected, so the star must not claim a pin that was never
  // saved.
  it('a failed PUT does not flip the star', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: [], pinnedStations: [] }), { status: 200 });
      }
      return new Response('no session', { status: 401 });
    });

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(2);
    });
    expect(screen.getByLabelText('Pin (currently not pinned)')).toBeInTheDocument();
  });
});
