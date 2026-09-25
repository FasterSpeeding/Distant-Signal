import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { BrowserMcpOAuthProvider } from '@/lib/mcpOAuthProvider';
import ChatCallbackPage from './page';

const mockAuth = vi.fn();
const mockReplace = vi.fn();

vi.mock('@modelcontextprotocol/sdk/client/auth.js', () => ({
  auth: (...args: unknown[]) => mockAuth(...args),
}));
// A single stable object, not `() => ({ replace: mockReplace })` --
// Next's own `useRouter()` returns a stable reference across renders in
// real usage, and returning a fresh object every call broke that
// assumption in a way that mattered: the page's own effect depends on
// nothing else, but a naively-exhaustive-deps version that DID include
// `router` would re-run (and, worse, re-`setState`) every render against
// an ever-changing dependency, which is exactly the infinite render loop
// this mock used to manufacture before `app/chat/callback/page.tsx`'s own
// effect was pinned to an empty dependency array for the same reason.
const mockRouter = { replace: mockReplace };
vi.mock('next/navigation', () => ({
  useRouter: () => mockRouter,
}));

function renderAt(search: string) {
  window.history.pushState({}, '', `/chat/callback${search}`);
  return renderWithMantine(<ChatCallbackPage />);
}

// Finding 3 of the deferred fapp Low-severity batch (2026-09-24 security
// review) added an OAuth `state` check the page now runs before ever
// calling `auth()`. Every test below that isn't specifically exercising
// that check needs a `state` param whose value actually matches what
// `BrowserMcpOAuthProvider.consumeAndVerifyState` has stored -- i.e. the
// same shape `provider.state()` itself would have produced before the
// authorization redirect that (in real usage) sent the browser here.
function renderAtWithValidState(search: string) {
  const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
  const state = provider.state();
  const separator = search.includes('?') ? '&' : '?';
  return renderAt(`${search}${separator}state=${state}`);
}

describe('ChatCallbackPage', () => {
  beforeEach(() => {
    localStorage.clear();
    mockAuth.mockReset();
    mockReplace.mockReset();
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('exchanges the code and redirects to /chat on success', async () => {
    mockAuth.mockResolvedValue('AUTHORIZED');
    renderAtWithValidState('?code=abc123');
    await waitFor(() => expect(mockReplace).toHaveBeenCalledWith('/chat'));
    expect(mockAuth).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ serverUrl: 'https://mcp.example.com', authorizationCode: 'abc123' }),
    );
  });

  it('shows "Connecting…" as the heading, with a spinner, while the exchange is in flight', () => {
    mockAuth.mockReturnValue(new Promise(() => {})); // never resolves
    renderAtWithValidState('?code=abc123');
    expect(screen.getByRole('heading', { name: 'Connecting…' })).toBeInTheDocument();
  });

  it('shows a "Connected…" heading on success, distinct from the connecting/error headings', async () => {
    mockAuth.mockResolvedValue('AUTHORIZED');
    renderAtWithValidState('?code=abc123');
    expect(await screen.findByRole('heading', { name: 'Connected, taking you to Chat…' })).toBeInTheDocument();
  });

  it('shows an error and does not redirect when no code is present', async () => {
    renderAt('');
    expect(await screen.findByRole('heading', { name: "Couldn't connect" })).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('shows a plain sentence and a "Back to Chat" button, not the raw message, as the error\'s primary content', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAtWithValidState('?code=abc123');
    expect(
      await screen.findByText("We couldn't finish connecting to the rail data service."),
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to Chat' })).toHaveAttribute('href', '/chat');
  });

  it('renders the error as a role="alert" with a non-colour icon (WCAG 1.4.1)', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAtWithValidState('?code=abc123');
    const alert = await screen.findByRole('alert');
    expect(alert.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  });

  it('demotes the raw exchange error to a collapsed <details>, not the primary explanation', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAtWithValidState('?code=abc123');
    const details = await screen.findByText('token exchange failed');
    const detailsEl = details.closest('details');
    expect(detailsEl).not.toBeNull();
    expect(detailsEl).not.toHaveAttribute('open');
  });

  it('shows an error when auth() returns REDIRECT instead of AUTHORIZED', async () => {
    mockAuth.mockResolvedValue('REDIRECT');
    renderAtWithValidState('?code=abc123');
    expect(await screen.findByText(/did not complete/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('gives up with a timeout error if the exchange never settles', () => {
    // `shouldAdvanceTime: true` -- same idiom as AutoRefresh.test.tsx's own
    // fake-timer setup.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockAuth.mockReturnValue(new Promise(() => {})); // never resolves
    renderAtWithValidState('?code=abc123');
    act(() => {
      vi.advanceTimersByTime(20_000);
    });
    expect(screen.getByRole('heading', { name: "Couldn't connect" })).toBeInTheDocument();
    expect(screen.getByText(/timed out/i)).toBeInTheDocument();
  });

  it('ignores a late resolution after the timeout has already fired', () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    // The exchange itself resolves 5s after this page's own 20s timeout --
    // both driven off the same fake clock in one `advanceTimersByTime`
    // call, rather than juggling fake timers and a manually-resolved
    // promise side by side.
    mockAuth.mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve('AUTHORIZED'), 25_000)),
    );
    renderAtWithValidState('?code=abc123');
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(screen.getByText(/timed out/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  // Finding 3 of the deferred fapp Low-severity batch (2026-09-24 security
  // review): this callback used to proceed straight to `auth()` with
  // whatever `code` the URL carried, with no OAuth `state` check of its
  // own at all -- defense against a planted/replayed authorization code
  // rested entirely on the PKCE verifier mismatching inside `auth()`'s own
  // token exchange. These confirm the new `state` check runs, and runs
  // BEFORE `auth()` is ever called.
  describe('OAuth state verification', () => {
    it('shows an error and never calls auth() when no state param is present at all', async () => {
      renderAt('?code=abc123');
      expect(await screen.findByRole('heading', { name: "Couldn't connect" })).toBeInTheDocument();
      expect(screen.getByText(/could not be verified/i)).toBeInTheDocument();
      expect(mockAuth).not.toHaveBeenCalled();
      expect(mockReplace).not.toHaveBeenCalled();
    });

    it('shows an error and never calls auth() when the state param does not match the stored value', async () => {
      // A real flow always stores one via `provider.state()` before the
      // redirect that leads back here -- simulate that, then have the
      // callback URL carry a different value (an attacker-planted or
      // stale/replayed one).
      const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
      provider.state();
      renderAt('?code=abc123&state=attacker-planted-state');
      expect(await screen.findByRole('heading', { name: "Couldn't connect" })).toBeInTheDocument();
      expect(screen.getByText(/could not be verified/i)).toBeInTheDocument();
      expect(mockAuth).not.toHaveBeenCalled();
    });

    it('proceeds to auth() when the state param matches the stored value', async () => {
      mockAuth.mockResolvedValue('AUTHORIZED');
      renderAtWithValidState('?code=abc123');
      await waitFor(() => expect(mockAuth).toHaveBeenCalled());
    });
  });
});
