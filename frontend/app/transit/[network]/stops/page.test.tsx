import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NETWORK, STOPS } from '@/test/transitFixtures';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import TransitStopsPage, { generateMetadata } from './page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTransitNetwork: vi.fn(), getTransitStops: vi.fn() };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  // `null`, as outside the app router: "Find a stop" keeps its props.
  useSearchParams: () => null,
}));

const params = (network: string, q?: string) => ({
  params: Promise.resolve({ network }),
  ...(q !== undefined && { searchParams: Promise.resolve({ q }) }),
});

describe('/transit/[network]/stops', () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitStops).mockResolvedValue(STOPS);
  });

  it('lists every stop A to Z, linking each, and back to the network', async () => {
    renderWithMantine(await TransitStopsPage(params('metrolink')));
    expect(screen.getByRole('heading', { level: 1, name: 'Manchester Metrolink stops' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Manchester Metrolink' }).getAttribute('href')).toBe('/transit/metrolink');
    const stops = screen.getByRole('list', { name: 'Stops' });
    expect(
      within(stops)
        .getAllByRole('link')
        .map((a) => a.textContent),
    ).toEqual(['Eccles', 'Piccadilly']);
    expect(within(stops).getByRole('link', { name: 'Piccadilly' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
  });

  it('filters by `?q=`, where the network page finder submits', async () => {
    renderWithMantine(await TransitStopsPage(params('metrolink', 'picc')));
    expect(screen.getByRole('textbox', { name: 'Find a stop' })).toHaveProperty('value', 'picc');
    const stops = screen.getByRole('list', { name: 'Stops' });
    expect(
      within(stops)
        .getAllByRole('link')
        .map((a) => a.textContent),
    ).toEqual(['Piccadilly']);
  });

  it('is not found for a network the api does not serve, or a malformed id', async () => {
    vi.mocked(api.getTransitNetwork).mockRejectedValue(new ApiNotFoundError('404'));
    await expect(TransitStopsPage(params('metrolink'))).rejects.toThrow('NEXT_NOT_FOUND');
    await expect(TransitStopsPage(params('Metro Link'))).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('titles the page', async () => {
    const meta = await generateMetadata(params('metrolink'));
    expect(meta.title).toBe('Manchester Metrolink stops');
  });
});
