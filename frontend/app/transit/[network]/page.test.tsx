import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { cardOf } from '@/test/clickableCard';
import {
  MINOR_DELAYS,
  NETWORK,
  NOTHING_REPORTED,
  SITUATIONS,
  STATUS_UNAVAILABLE,
  STOPS,
  TFL_NETWORK,
} from '@/test/transitFixtures';
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
const redirectMock = vi.fn((to: string) => {
  throw new Error(`NEXT_REDIRECT ${to}`);
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  redirect: (to: string) => redirectMock(to),
  useRouter: () => ({ push: vi.fn() }),
}));
// A plain GET form: `next/form` needs the app router.
vi.mock('next/form', () => ({
  default: ({ prefetch: _prefetch, ...props }: React.ComponentProps<'form'> & { prefetch?: boolean }) => (
    <form method="get" {...props} />
  ),
}));

const params = (network: string) => ({ params: Promise.resolve({ network }) });

describe('/transit/[network]', () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitStops).mockResolvedValue(STOPS);
    vi.mocked(api.getTransitSituations).mockResolvedValue({ state: 'none', asOf: null, current: [], upcoming: [] });
  });

  it('lists the lines and finds stops, without listing or fetching them', async () => {
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.getByRole('heading', { level: 1, name: 'Manchester Metrolink' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Blue Line' }).getAttribute('href')).toBe('/transit/metrolink/lines/blue');
    // A finder, not a list: no stop is on the page, and none was fetched.
    expect(screen.getByRole('combobox', { name: 'Find a stop' })).toBeTruthy();
    expect(screen.queryByRole('list', { name: 'Stops' })).toBeNull();
    expect(screen.queryByRole('link', { name: 'Piccadilly' })).toBeNull();
    expect(api.getTransitStops).not.toHaveBeenCalled();
    expect(screen.getByRole('link', { name: 'All stops, A to Z' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops',
    );
    expect(screen.getByText(/We don't have live tram times/)).toBeTruthy();
    expect(screen.getByText(/Open Government Licence/)).toBeTruthy();
  });

  it('sends the old in-page search (`?q=`) to the A to Z', async () => {
    await expect(
      TransitNetworkPage({ ...params('metrolink'), searchParams: Promise.resolve({ q: 'picc' }) }),
    ).rejects.toThrow('NEXT_REDIRECT /transit/metrolink/stops?q=picc');
  });

  it("fetches the TfL network's stops for nothing", async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue(TFL_NETWORK);
    renderWithMantine(await TransitNetworkPage(params('tfl')));
    expect(api.getTransitStops).not.toHaveBeenCalled();
    expect(api.getTransitSituations).toHaveBeenCalled();
    expect(screen.getByRole('combobox', { name: 'Find a stop' })).toBeTruthy();
  });

  it('shows no status or disruptions while the network has no disruptions feed', async () => {
    vi.mocked(api.getTransitSituations).mockResolvedValue({ state: 'none', asOf: null, current: [], upcoming: [] });
    renderWithMantine(await TransitNetworkPage(params('metrolink')));
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
    expect(screen.queryByText(/reported disruption|Status unavailable/)).toBeNull();
  });

  it("groups the TfL network's lines by mode, with TfL's status and no disruptions list", async () => {
    vi.mocked(api.getTransitNetwork).mockResolvedValue(TFL_NETWORK);
    renderWithMantine(await TransitNetworkPage(params('tfl')));
    expect(screen.getByText(/Each stop's page has live arrivals/)).toBeTruthy();
    expect(screen.queryByText(/We don't have live tram times/)).toBeNull();
    const headings = screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
    expect(headings.slice(0, 3)).toEqual(['Underground', 'DLR', 'Also run by TfL']);
    expect(cardOf(screen.getByRole('link', { name: 'Victoria' })).textContent).toContain('Minor delays');
    expect(screen.getByRole('link', { name: 'Elizabeth line' }).getAttribute('href')).toBe('/lines/elizabeth-line');
    expect(screen.queryByRole('heading', { name: 'Disruptions' })).toBeNull();
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
    expect(cardOf(screen.getByRole('link', { name: 'Blue Line' })).textContent).toContain('Minor delays');
    expect(cardOf(screen.getByRole('link', { name: 'Green Line' })).textContent).toContain('No reported disruption');
    expect(screen.getByRole('heading', { name: 'Disruptions' })).toBeTruthy();
    // The delayed line's card says why in a line; the disruption list has it too.
    expect(cardOf(screen.getByRole('link', { name: 'Blue Line' })).textContent).toContain(
      SITUATIONS.current[0]!.summary,
    );
    expect(cardOf(screen.getByRole('link', { name: 'Green Line' })).querySelector('[data-line-summary]')).toBeNull();
    expect(
      within(screen.getByRole('list', { name: 'Disruptions now' })).getByText(SITUATIONS.current[0]!.summary),
    ).toBeTruthy();
    expect(screen.getByText(SITUATIONS.upcoming[0]!.summary)).toBeTruthy();
    // The lift notice at Piccadilly links the stop: the one case that
    // fetches the stops, server-side, to name it.
    expect(api.getTransitStops).toHaveBeenCalledWith('metrolink');
    const now = screen.getByRole('list', { name: 'Disruptions now' });
    expect(within(now).getByRole('link', { name: 'Piccadilly' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
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
    expect(cardOf(screen.getByRole('link', { name: 'Blue Line' })).textContent).toContain('Status unavailable');
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

  it("doesn't call the TfL network's Tube and DLR lines trams", async () => {
    // Victoria, the DLR and Central: no tram line, so no tram anywhere.
    vi.mocked(api.getTransitNetwork).mockResolvedValue(TFL_NETWORK);
    vi.mocked(api.getTransitStops).mockResolvedValue([]);
    renderWithMantine(await TransitNetworkPage(params('tfl')));
    expect(document.body.textContent).not.toMatch(/tram/i);
    const meta = await generateMetadata(params('tfl'));
    expect(meta.description).toBe('London (TfL): Tube and DLR lines, stops and scheduled times.');
    expect(JSON.stringify(meta)).not.toMatch(/tram/i);
  });

  it('titles the page with the network name, and describes it by its modes', async () => {
    const meta = await generateMetadata(params('metrolink'));
    expect(meta.title).toBe('Manchester Metrolink');
    expect(meta.description).toBe('Manchester Metrolink: tram lines, stops and scheduled times.');
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
    expect(screen.getByRole('heading', { level: 1, name: 'Trams and metros' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Manchester Metrolink' }).getAttribute('href')).toBe('/transit/metrolink');
  });

  it('says what the TfL network runs', async () => {
    vi.mocked(api.getTransitNetworks).mockResolvedValue([NETWORK, TFL_NETWORK]);
    renderWithMantine(await TransitIndexPage());
    expect(cardOf(screen.getByRole('link', { name: 'London (TfL)' })).textContent).toContain(
      'Underground, DLR and London Trams',
    );
  });
});
