import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConnectAppConsent } from './ConnectAppConsent';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/account/connected-apps/ds-mcp',
  useSearchParams: () => new URLSearchParams(''),
}));

const consent = {
  client: {
    name: 'ds-mcp',
    displayName: 'Distant Signal assistant',
    scopes: ['journeys:read', 'journeys:write', 'journeys:delete'],
  },
  username: 'alice',
  name: 'Alice',
  confirmationToken: 'one-time-token',
  expiresAt: '2026-10-14T10:10:00Z',
  existingGrant: null,
};

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

describe('ConnectAppConsent', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows who is signed in and what the app could do, then grants only the ticked scopes', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(json(consent))
      .mockResolvedValueOnce(json({ client: 'ds-mcp' }, 201));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<ConnectAppConsent client="ds-mcp" requestedScopes="journeys:read journeys:write" />);

    expect(await screen.findByText('You are logged in to Distant Signal as Alice.')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/account/connected-apps/ds-mcp/consent',
      expect.objectContaining({ cache: 'no-store' }),
    );
    // The link preselected two of the three boxes.
    expect(screen.getByRole('checkbox', { name: 'See your saved journeys' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'Save journeys to your account' })).toBeChecked();
    const deleteBox = screen.getByRole('checkbox', { name: 'Delete journeys it saved, unless you have shared them' });
    expect(deleteBox).not.toBeChecked();
    // The visitor unticks one: the grant takes what is ticked now.
    fireEvent.click(screen.getByRole('checkbox', { name: 'Save journeys to your account' }));
    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));

    expect(await screen.findByText('Distant Signal assistant is connected')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenLastCalledWith('/api/account/connected-apps/ds-mcp', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ scopes: ['journeys:read'], confirmationToken: 'one-time-token' }),
    });
  });

  it('asks for a reload when the confirmation token has expired', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce(json(consent))
        .mockResolvedValueOnce(json({ error: 'confirmation_required', message: 'confirm' }, 403)),
    );
    renderWithMantine(<ConnectAppConsent client="ds-mcp" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Allow' }));
    expect(await screen.findByText('This page has expired. Reload it and try again.')).toBeInTheDocument();
  });

  it('says so for an app that does not exist or is turned off', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(json({ error: 'not_found' }, 404)));
    renderWithMantine(<ConnectAppConsent client="nope" />);
    expect(await screen.findByText("That app isn't available")).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Allow' })).not.toBeInTheDocument();
  });

  it('cannot allow with nothing ticked', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValueOnce(json({ ...consent, client: { ...consent.client, scopes: ['journeys:read'] } })),
    );
    renderWithMantine(<ConnectAppConsent client="ds-mcp" />);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'See your saved journeys' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Allow' })).toBeDisabled());
  });
});
