import { describe, it, expect, afterEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConnectedAppNotice } from './ConnectedAppNotice';
import type { ConnectedAppGrant } from '@/lib/types';

const notice: ConnectedAppGrant = {
  client: 'ds-mcp',
  displayName: 'Distant Signal assistant',
  scopes: ['journeys:read'],
  createdAt: '2026-10-14T09:00:00Z',
  lastUsedAt: null,
  clientEnabled: true,
};

describe('ConnectedAppNotice', () => {
  afterEach(() => {
    window.localStorage.clear();
  });

  it('names the app, when it was connected, and where to disconnect it', async () => {
    renderWithMantine(<ConnectedAppNotice notice={notice} />);
    expect(await screen.findByText('Distant Signal assistant was connected to your account')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Disconnect it in Connected apps' })).toHaveAttribute(
      'href',
      '/account/connected-apps',
    );
  });

  it('stays dismissed for that connection, and shows again for a new one', async () => {
    const { unmount } = renderWithMantine(<ConnectedAppNotice notice={notice} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByText('Distant Signal assistant was connected to your account')).not.toBeInTheDocument();
    unmount();
    renderWithMantine(<ConnectedAppNotice notice={notice} />);
    expect(screen.queryByText('Distant Signal assistant was connected to your account')).not.toBeInTheDocument();
    renderWithMantine(<ConnectedAppNotice notice={{ ...notice, createdAt: '2026-10-15T09:00:00Z' }} />);
    expect(await screen.findByText('Distant Signal assistant was connected to your account')).toBeInTheDocument();
  });
});
