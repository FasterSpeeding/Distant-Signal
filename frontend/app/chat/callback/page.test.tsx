import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
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
    renderAt('?code=abc123');
    await waitFor(() => expect(mockReplace).toHaveBeenCalledWith('/chat'));
    expect(mockAuth).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ serverUrl: 'https://mcp.example.com', authorizationCode: 'abc123' }),
    );
  });

  it('shows "Connecting…" as the heading, with a spinner, while the exchange is in flight', () => {
    mockAuth.mockReturnValue(new Promise(() => {})); // never resolves
    renderAt('?code=abc123');
    expect(screen.getByRole('heading', { name: 'Connecting…' })).toBeInTheDocument();
  });

  it('shows a "Connected…" heading on success, distinct from the connecting/error headings', async () => {
    mockAuth.mockResolvedValue('AUTHORIZED');
    renderAt('?code=abc123');
    expect(await screen.findByRole('heading', { name: 'Connected, taking you to Chat…' })).toBeInTheDocument();
  });

  it('shows an error and does not redirect when no code is present', async () => {
    renderAt('');
    expect(await screen.findByRole('heading', { name: "Couldn't connect" })).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('shows a plain sentence and a "Back to Chat" button, not the raw message, as the error\'s primary content', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAt('?code=abc123');
    expect(
      await screen.findByText("We couldn't finish connecting to the rail data service."),
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to Chat' })).toHaveAttribute('href', '/chat');
  });

  it('renders the error as a role="alert" with a non-colour icon (WCAG 1.4.1)', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAt('?code=abc123');
    const alert = await screen.findByRole('alert');
    expect(alert.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  });

  it('demotes the raw exchange error to a collapsed <details>, not the primary explanation', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAt('?code=abc123');
    const details = await screen.findByText('token exchange failed');
    const detailsEl = details.closest('details');
    expect(detailsEl).not.toBeNull();
    expect(detailsEl).not.toHaveAttribute('open');
  });

  it('shows an error when auth() returns REDIRECT instead of AUTHORIZED', async () => {
    mockAuth.mockResolvedValue('REDIRECT');
    renderAt('?code=abc123');
    expect(await screen.findByText(/did not complete/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('gives up with a timeout error if the exchange never settles', () => {
    // `shouldAdvanceTime: true` -- same idiom as AutoRefresh.test.tsx's own
    // fake-timer setup.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockAuth.mockReturnValue(new Promise(() => {})); // never resolves
    renderAt('?code=abc123');
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
    renderAt('?code=abc123');
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(screen.getByText(/timed out/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });
});
