import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { MINOR_DELAYS, NETWORK, NOTHING_REPORTED, SITUATIONS, STATUS_UNAVAILABLE, STOPS } from '@/test/transitFixtures';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import TransitNetworkPage, { generateMetadata } from './page';
import TransitIndexPage from '../page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getTransitNetworks: vi.fn(),
    getTransitNetwork: vi.fn(),
    getTransitStops: vi.fn(),
    getTransitSituations: vi.fn(),
  };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
}));

const params = (network: string) => ({ params: Promise.resolve({ network }) });

describe('/transit/[network]', () => {
  beforeEach(() => {
    cleanup();
    notFoundMock.mockClear();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitStops).mockResolvedValue(STOPS);
    vi.mocked(api.getTransitSituations).mockResolvedValue({ state: 'none', asOf: null, current: [], upcoming: [] });
  });

  it('lists the lines and stops, with the data credit', async () => {
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.getByRole('heading', { level: 1, name: 'Manchester Metrolink' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Blue Line' }).getAttribute('href')).toBe('/transit/metrolink/lines/blue');
    expect(screen.getByRole('link', { name: 'Piccadilly' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
    expect(screen.getByText(/We don't have live tram times/)).toBeTruthy();
    expect(screen.getByText(/Open Government Licence/)).toBeTruthy();
  });

  it('shows no status or disruptions while the network has no disruptions feed', async () => {
    vi.mocked(api.getTransitSituations).mockResolvedValue({ state: 'none', asOf: null, current: [], upcoming: [] });
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
    expect(screen.queryByText(/reported disruption|Status unavailable/)).toBeNull();
  });

  it("shows each line's status and the network's disruptions", async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue({
      ...NETWORK,
      status: { state: 'current', asOf: SITUATIONS.asOf },
      lines: [
        { ...NETWORK.lines[0]!, status: MINOR_DELAYS },
        { ...NETWORK.lines[1]!, status: NOTHING_REPORTED },
      ],
    });
    vi.mocked(api.getTransitSituations).mockResolvedValue(SITUATIONS);
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.getByRole('link', { name: /Blue Line/ }).textContent).toContain('Minor delays');
    expect(screen.getByRole('link', { name: /Green Line/ }).textContent).toContain('No reported disruption');
    expect(screen.getByRole('heading', { name: 'Disruptions' })).toBeTruthy();
    expect(screen.getByText(SITUATIONS.current[0]!.summary)).toBeTruthy();
    expect(screen.getByText(SITUATIONS.upcoming[0]!.summary)).toBeTruthy();
  });

  it('says when the disruptions could not be loaded, and when they are stale', async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue({
      ...NETWORK,
      status: { state: 'current', asOf: SITUATIONS.asOf },
    });
    vi.mocked(api.getTransitSituations).mockRejectedValue(new Error('500'));
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.getByText(/^Couldn't load disruptions\./)).toBeTruthy();
    cleanup();
    vi.mocked(api.getTransitNetwork).mockResolvedValue({
      ...NETWORK,
      status: { state: 'unavailable', asOf: STATUS_UNAVAILABLE.asOf },
      lines: [{ ...NETWORK.lines[0]!, status: STATUS_UNAVAILABLE }],
    });
    vi.mocked(api.getTransitSituations).mockResolvedValue({
      ...SITUATIONS,
      state: 'unavailable',
      asOf: STATUS_UNAVAILABLE.asOf,
      current: [],
      upcoming: [],
    });
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.getByRole('link', { name: /Blue Line/ }).textContent).toContain('Status unavailable');
    expect(screen.getByText('Status unavailable. Last read at 09:00.')).toBeTruthy();
  });

  it('is not found for a network the api does not serve', async () => {
    vi.mocked(api.getTransitNetwork).mockRejectedValue(new ApiNotFoundError('404'));
    await expect(TransitNetworkPage(params('metrolink'))).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('is not found for a malformed id without asking the api', async () => {
    await expect(TransitNetworkPage(params('Metro Link'))).rejects.toThrow('NEXT_NOT_FOUND');
    expect(api.getTransitNetwork).not.toHaveBeenCalledWith('Metro Link');
  });

  it('titles the page with the network name', async () => {
    const meta = await generateMetadata(params('metrolink'));
    expect(meta.title).toBe('Manchester Metrolink');
  });
});

describe('/transit', () => {
  beforeEach(() => {
    cleanup();
    notFoundMock.mockClear();
  });

  it('is not found while no network is served (the default)', async () => {
    vi.mocked(api.getTransitNetworks).mockResolvedValue([]);
    await expect(TransitIndexPage()).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('lists the served networks', async () => {
    vi.mocked(api.getTransitNetworks).mockResolvedValue([NETWORK]);
    renderWithMantine(await TransitIndexPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Trams' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Manchester Metrolink' }).getAttribute('href')).toBe('/transit/metrolink');
  });
});
