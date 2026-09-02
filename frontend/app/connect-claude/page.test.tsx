import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
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
  });

  it('shows a login prompt when not authenticated', async () => {
    vi.mocked(api.getSession).mockResolvedValue(loggedOut());
    renderWithMantine(await ConnectClaudePage());
    expect(screen.getAllByText(/log in/i).length).toBeGreaterThan(0);
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
});
