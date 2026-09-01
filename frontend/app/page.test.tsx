import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import DashboardPage from './page';
import * as api from '@/lib/api';
import type { LineStatusReport, TrackedTrainListItem } from '@/lib/types';

vi.mock('@/lib/api');

function report(overrides: Partial<LineStatusReport> = {}): LineStatusReport {
  return {
    $type: 'x', id: 'bakerloo', name: 'Bakerloo', modeName: 'tube', operators: [],
    lineStatuses: [{ statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: '' } as never],
    computedAt: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

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

describe('DashboardPage', () => {
  it('anonymous, all lines good: shows the no-disruption message, not a raw empty state', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in to pin your lines and stations' })).toHaveAttribute(
      'href', '/api/auth/login',
    );
  });

  it('anonymous, a line disrupted: lists it, worst-first', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'central', name: 'Central', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '' } as never] }),
      report(),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('anonymous: merged TfL counterpart ids are excluded from the widget', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '' } as never] }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
  });

  it('logged in: renders the existing pinned-lines/pinned-stations behavior, not the anonymous branch', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: ['central'], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    expect(screen.queryByText(/Right now/)).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in to pin your lines and stations' })).not.toBeInTheDocument();
  });

  it('logged in, an auth glitch (getSession rejects): degrades to the anonymous branch, not a crash', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('boom'));
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: 'Log in to pin your lines and stations' })).toBeInTheDocument();
  });
});

// Minimal, shared stubs for the two pre-existing fetches this page also
// makes -- not under test here, just enough for the page to render without
// throwing. Scoped to this describe block only (not global): the
// `DashboardPage` describe block above sets its own explicit mocks per
// test and doesn't need these defaults.
//
// `getSession` is explicitly re-mocked to a logged-in user here too --
// without this, the last test in the `DashboardPage` describe block above
// leaves `getSession` mocked to a *rejected* promise (its own
// auth-glitch-degrades-to-anonymous test), and since Vitest doesn't reset
// mocks between tests by default, that rejection would otherwise leak into
// every test below and force the anonymous branch, which never renders the
// Your Tracked Trains section this whole describe block exists to test.
describe('DashboardPage -- Your Tracked Trains section', () => {
  beforeEach(() => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
  });

  it('getMyTrackedTrains() returns null (logged out): section absent, other two sections unchanged', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Stations' })).toBeInTheDocument();
  });

  it('getMyTrackedTrains() returns [] (logged in, nothing tracked): section absent', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
  });

  it('populated list: section present with a "View all" link to /track/mine', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Tracked Trains' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/track/mine');
  });

  it('more than 5 tracked trains: only the first 5 (as returned) are rendered', async () => {
    const trains = Array.from({ length: 7 }, (_, i) => item({ id: i + 1, pinOriginCrs: `T${i + 1}` }));
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(trains);
    renderWithMantine(await DashboardPage());
    for (let i = 1; i <= 5; i++) {
      expect(screen.getByText(new RegExp(`^T${i}`))).toBeInTheDocument();
    }
    expect(screen.queryByText(/^T6/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^T7/)).not.toBeInTheDocument();
  });

  it('rows render in the order getMyTrackedTrains returned them (no client-side re-sort)', async () => {
    const first = item({ id: 1, pinOriginCrs: 'WAT' });
    const second = item({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    renderWithMantine(await DashboardPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent ?? '')
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ delayMinutes: 12 })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/C21373/2026-08-31');
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });
});
