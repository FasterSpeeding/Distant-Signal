import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NETWORK, STOPS } from '@/test/transitFixtures';
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
