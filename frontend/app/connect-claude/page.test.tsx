import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ConnectClaudePage, { metadata } from './page';

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

  // Since the consent bridge's retirement there is no Distant Signal
  // session in the flow, so the page no longer gates on one (no
  // getSession() mock needed: the page never calls the backend).
  it('shows the connector URL and step-by-step instructions to everyone', () => {
    renderWithMantine(ConnectClaudePage());
    expect(screen.getByText(/Customize/)).toBeInTheDocument();
    expect(screen.getByText(/Add custom connector/i)).toBeInTheDocument();
    expect(screen.getByText('https://mcp.example.com')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Log in' })).not.toBeInTheDocument();
  });

  it('describes the sign-in step without the retired confirmation screen', () => {
    renderWithMantine(ConnectClaudePage());
    expect(screen.getByText(/sends you to the sign-in page/)).toBeInTheDocument();
    expect(screen.queryByText(/confirm the connection/)).not.toBeInTheDocument();
  });

  it('falls back to a placeholder when NEXT_PUBLIC_RAILMCP_PUBLIC_URL is unset (railMcp not enabled on this deployment)', () => {
    vi.unstubAllEnvs();
    renderWithMantine(ConnectClaudePage());
    expect(screen.getByText('(not configured on this deployment)')).toBeInTheDocument();
  });

  // Review §3.1.6.
  it('offers a "Copy connector URL" button for the long connector URL', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    renderWithMantine(ConnectClaudePage());
    fireEvent.click(screen.getByRole('button', { name: 'Copy connector URL' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('https://mcp.example.com'));
  });

  it("renders the plan-requirement Alert in grape, not Mantine's default blue (blue is reserved for planned-severity)", () => {
    renderWithMantine(ConnectClaudePage());
    // Mantine encodes an Alert's color as CSS custom properties on its
    // root's inline `style`, not as a colour name in its `className`.
    const alert = screen.getByRole('alert');
    expect(alert).toHaveAttribute('data-variant', 'light');
    expect(alert).toHaveStyle({ '--alert-bg': 'var(--mantine-color-grape-light)' });
  });

  it('says "Click the + button", not the terser "Click +"', () => {
    renderWithMantine(ConnectClaudePage());
    expect(screen.getByText(/Click the/)).toBeInTheDocument();
    expect(screen.queryByText(/^Click \+/)).not.toBeInTheDocument();
  });

  it('uses an em dash rather than a literal "--" in its copy', () => {
    const { container } = renderWithMantine(ConnectClaudePage());
    // MantineProvider injects its own `<style>` tags full of `--mantine-*`
    // CSS custom properties into the container -- strip those before
    // checking the page's own rendered copy, or every render of this test
    // would fail on Mantine's own CSS variables, not this page's text.
    const clone = container.cloneNode(true) as HTMLElement;
    clone.querySelectorAll('style').forEach((el) => el.remove());
    expect(clone.textContent).not.toMatch(/--/);
    expect(clone.textContent).toMatch(/—/);
  });
  it('names the tab after the page, not just the site', () => {
    expect(metadata.title).toBe('Connect Claude — Distant Signal');
  });

  it('has one h1, and puts the steps in a "How to connect" section headed by an h2', () => {
    renderWithMantine(ConnectClaudePage());
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1);
    expect(screen.getByRole('heading', { level: 1, name: 'Connect Claude to Distant Signal' })).toBeInTheDocument();
    const section = screen.getByRole('region', { name: 'How to connect' });
    expect(within(section).getByRole('heading', { level: 2, name: 'How to connect' })).toBeInTheDocument();
    expect(within(section).getAllByRole('listitem')).toHaveLength(4);
  });

  it('lets the long connector URL wrap instead of running off a phone-width screen', () => {
    renderWithMantine(ConnectClaudePage());
    expect(screen.getByText('https://mcp.example.com')).toHaveStyle({ wordBreak: 'break-all' });
  });
});
