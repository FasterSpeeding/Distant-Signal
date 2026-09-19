import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ConnectClaudePage from './page';
import * as api from '@/lib/api';
import type { SessionInfo } from '@/lib/types';

vi.mock('@/lib/api');
// LoginLink calls usePathname()/useSearchParams() -- same stub this app's
// other not-logged-in-nudge tests use (e.g. app/track/mine/page.test.tsx),
// since those hooks throw outside an app router context.
vi.mock('next/navigation', () => ({
  usePathname: () => '/connect-claude',
  useSearchParams: () => new URLSearchParams(''),
}));

// jsdom doesn't implement `navigator.clipboard` -- same stub pattern
// ShareButton.test.tsx uses, trimmed to just the one method Mantine's
// `CopyButton` actually calls.
function stubClipboard(writeText: ReturnType<typeof vi.fn>) {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    writable: true,
    configurable: true,
  });
}

function loggedOut(): SessionInfo {
  return { authenticated: false, id: null, email: null, name: null };
}

function loggedIn(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return { authenticated: true, id: 'user-1', email: 'rider@example.com', name: 'Ada Rider', ...overrides };
}

describe('/connect-claude', () => {
  beforeEach(() => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    // @ts-expect-error -- deliberately removing a property TS believes is
    // always present on Navigator, same as ShareButton.test.tsx's own
    // cleanup.
    delete navigator.clipboard;
  });

  it('shows a login prompt when not authenticated', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedOut());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.getAllByText(/log in/i).length).toBeGreaterThan(0);
  });

  // Review §2.16: this used to be an underlined text link -- promoted to a
  // filled button so the page's one anonymous action doesn't read as the
  // weakest thing on it.
  it('renders the login prompt as a filled button, not a plain text link', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedOut());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.getByRole('button', { name: 'Log in' })).toBeInTheDocument();
  });

  it('does not show the connector URL when not authenticated', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedOut());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.queryByText('https://mcp.example.com')).not.toBeInTheDocument();
  });

  it('shows the connector URL and step-by-step instructions when authenticated', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.getByText(/Customize/)).toBeInTheDocument();
    expect(screen.getByText(/Add custom connector/i)).toBeInTheDocument();
    expect(screen.getByText('https://mcp.example.com')).toBeInTheDocument();
  });

  it('falls back to a placeholder when NEXT_PUBLIC_RAILMCP_PUBLIC_URL is unset (railMcp not enabled on this deployment)', async () => {
    vi.unstubAllEnvs();
    vi.mocked(api.getSession).mockResolvedValue(loggedIn());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.getByText('(not configured on this deployment)')).toBeInTheDocument();
  });

  // Review §3.1.6.
  it('offers a "Copy connector URL" button for the long connector URL', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    vi.mocked(api.getSession).mockResolvedValue(loggedIn());
    renderWithMantine(await ConnectClaudePage());
    fireEvent.click(screen.getByRole('button', { name: 'Copy connector URL' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('https://mcp.example.com'));
  });

  it('renders the plan-requirement Alert in grape, not Mantine\'s default blue (blue is reserved for planned-severity)', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn());
    renderWithMantine(await ConnectClaudePage());
    // Mantine encodes an Alert's color as CSS custom properties on its
    // root's inline `style`, not as a colour name in its `className`.
    const alert = screen.getByRole('alert');
    expect(alert).toHaveAttribute('data-variant', 'light');
    expect(alert).toHaveStyle({ '--alert-bg': 'var(--mantine-color-grape-light)' });
  });

  it('says "Click the + button", not the terser "Click +"', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.getByText(/Click the/)).toBeInTheDocument();
    expect(screen.queryByText(/^Click \+/)).not.toBeInTheDocument();
  });

  it('uses an em dash rather than a literal "--" in its copy', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedIn());
    const { container } = renderWithMantine(await ConnectClaudePage());
    // MantineProvider injects its own `<style>` tags full of `--mantine-*`
    // CSS custom properties into the container -- strip those before
    // checking the page's own rendered copy, or every render of this test
    // would fail on Mantine's own CSS variables, not this page's text.
    const clone = container.cloneNode(true) as HTMLElement;
    clone.querySelectorAll('style').forEach((el) => el.remove());
    expect(clone.textContent).not.toMatch(/--/);
    expect(clone.textContent).toMatch(/—/);
  });
});
