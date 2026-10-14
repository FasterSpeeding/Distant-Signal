import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConnectedApps } from './ConnectedApps';
import type { ConnectedAppsData } from '@/lib/types';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: refreshMock }),
  usePathname: () => '/account/connected-apps',
  useSearchParams: () => new URLSearchParams(''),
}));

const data: ConnectedAppsData = {
  grants: [
    {
      client: 'ds-mcp',
      displayName: 'Distant Signal assistant',
      scopes: ['journeys:read', 'journeys:write'],
      createdAt: '2026-10-14T09:00:00Z',
      lastUsedAt: null,
      clientEnabled: true,
    },
  ],
  available: [
    { name: 'ds-mcp', displayName: 'Distant Signal assistant', scopes: ['journeys:read'] },
    { name: 'other-app', displayName: 'Other app', scopes: ['journeys:read'] },
  ],
  recentDeletions: [
    {
      id: 1,
      client: 'ds-mcp',
      journeyId: 42,
      deletedAt: '2026-10-13T09:00:00Z',
      summary: {
        customName: 'Weekend away',
        legs: [{ serviceDate: '2026-10-20', originCrs: 'EUS', destinationCrs: 'MKC', trainUid: 'C11052' }],
      },
    },
  ],
};

describe('ConnectedApps', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('lists each connected app with what it may do and when it was used', () => {
    renderWithMantine(<ConnectedApps data={data} />);
    expect(screen.getByRole('heading', { name: 'Connected apps' })).toBeInTheDocument();
    expect(screen.getByText('Distant Signal assistant')).toBeInTheDocument();
    expect(screen.getByText('See your saved journeys')).toBeInTheDocument();
    expect(screen.getByText('Save journeys to your account')).toBeInTheDocument();
    expect(screen.getByText(/Not used yet\./)).toBeInTheDocument();
    // Only apps not already connected are offered.
    expect(screen.getByRole('link', { name: 'Connect Other app' })).toHaveAttribute(
      'href',
      '/account/connected-apps/other-app',
    );
    expect(screen.queryByRole('link', { name: 'Connect Distant Signal assistant' })).not.toBeInTheDocument();
  });

  it('lists what an app deleted, so it can be saved again', () => {
    renderWithMantine(<ConnectedApps data={data} />);
    expect(screen.getByRole('heading', { name: 'Recently deleted by the assistant' })).toBeInTheDocument();
    expect(screen.getByText('Weekend away')).toBeInTheDocument();
    expect(screen.getByText('EUS to MKC, on 20 Oct 2026, train C11052')).toBeInTheDocument();
    expect(screen.getByText(/Deleted by Distant Signal assistant on 13 Oct 2026\./)).toBeInTheDocument();
  });

  it('disconnects after a confirmation', async () => {
    renderWithMantine(<ConnectedApps data={data} />);
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }));
    expect(fetch).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole('button', { name: 'Confirm disconnect Distant Signal assistant' }));
    await waitFor(() => expect(fetch).toHaveBeenCalledWith('/api/account/connected-apps/ds-mcp', { method: 'DELETE' }));
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('says when nothing is connected or deleted, and notes an app that is turned off', () => {
    renderWithMantine(
      <ConnectedApps
        data={{
          grants: [{ ...data.grants[0]!, clientEnabled: false, lastUsedAt: '2026-10-14T10:00:00Z' }],
          available: [],
          recentDeletions: [],
        }}
        showTitle={false}
      />,
    );
    expect(screen.queryByRole('heading', { name: 'Connected apps' })).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Connected apps' })).toBeInTheDocument();
    expect(screen.getByText(/turned it off for now/)).toBeInTheDocument();
    expect(screen.getByText(/Last used 14 Oct 2026/)).toBeInTheDocument();
    expect(screen.getByText('Nothing in the last 30 days.')).toBeInTheDocument();
    renderWithMantine(<ConnectedApps data={{ grants: [], available: [], recentDeletions: [] }} />);
    expect(screen.getByText('No apps are connected.')).toBeInTheDocument();
  });
});
