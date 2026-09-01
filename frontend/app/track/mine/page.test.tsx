import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import MyTrackedTrainsPage from './page';
import * as api from '@/lib/api';
import type { TrackedTrainListItem } from '@/lib/types';

vi.mock('@/lib/api');

function item(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    ...overrides,
  };
}

describe('MyTrackedTrainsPage', () => {
  it('null (not logged in): shows a login nudge', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(
      screen.getByRole('link', { name: "Log in to see the trains you're tracking" }),
    ).toHaveAttribute('href', '/api/auth/login');
  });

  it('empty array: shows the empty state with a working link to /track', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/haven't tracked any trains yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a train' })).toHaveAttribute('href', '/track');
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute(
      'href',
      '/train/C21373/2026-08-31',
    );
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('resolved with a null trainUid (defensive case): falls back to the by-id route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ trainUid: null })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ delayMinutes: 12 })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  it('renders the resolutionStatus badge for a pending/unresolved train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'unresolved', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Unmatched')).toBeInTheDocument();
  });

  it('origin-only train (no destination): renders just the origin CRS, no arrow', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ pinDestinationCrs: null })]);
    renderWithMantine(await MyTrackedTrainsPage());
    const link = screen.getByRole('link', { name: /^WAT/ });
    expect(link).toBeInTheDocument();
    expect(link.textContent).not.toContain('→');
  });

  it('renders rows in the same order getMyTrackedTrains returned them', async () => {
    const first = item({ id: 1, pinOriginCrs: 'WAT' });
    const second = item({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    renderWithMantine(await MyTrackedTrainsPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent ?? '')
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });
});
