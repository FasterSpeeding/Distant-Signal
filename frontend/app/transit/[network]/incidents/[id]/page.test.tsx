import { describe, it, expect, vi, beforeEach } from 'vitest';
import { cleanup, screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DELAY_DETAIL, LIFT, NETWORK, STOPS } from '@/test/transitFixtures';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { TransitSituationDetail } from '@/lib/types';
import TransitIncidentPage, { generateMetadata } from './page';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTransitNetwork: vi.fn(), getTransitSituation: vi.fn(), getTransitStops: vi.fn() };
});

const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
}));

const params = (id: string, network = 'metrolink') => ({ params: Promise.resolve({ network, id }) });

/** The lift notice's page: a stop notice, so no line is affected. */
const LIFT_DETAIL: TransitSituationDetail = {
  ...DELAY_DETAIL,
  ...LIFT,
  severity: 26,
  severityDescription: 'Information',
  affects: [],
};

describe('/transit/[network]/incidents/[id]', () => {
  beforeEach(() => {
    cleanup();
    notFoundMock.mockClear();
    vi.mocked(api.getTransitNetwork).mockResolvedValue(NETWORK);
    vi.mocked(api.getTransitSituation).mockResolvedValue(DELAY_DETAIL);
    vi.mocked(api.getTransitStops).mockResolvedValue(STOPS);
  });

  it('shows an active disruption: its status, text, advice and the lines it affects', async () => {
    renderWithMantine(await TransitIncidentPage(params('S1')));
    expect(api.getTransitSituation).toHaveBeenCalledWith('metrolink', 'S1');
    expect(screen.getByRole('heading', { level: 1, name: DELAY_DETAIL.summary })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Back to Manchester Metrolink' }).getAttribute('href')).toBe(
      '/transit/metrolink',
    );
    expect(screen.getByText('Active')).toBeTruthy();
    // Plain text, as published: markup shows as text, never HTML.
    expect(screen.getByText('A <b>fault</b> at Cornbrook.')).toBeTruthy();
    expect(screen.getByText('Allow extra time.')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'More details from Manchester Metrolink' }).getAttribute('href')).toBe(
      'https://example.invalid/s1',
    );
    const affected = screen.getByRole('list', { name: 'Lines affected' });
    expect(within(affected).getByRole('link', { name: 'Blue Line' }).getAttribute('href')).toBe(
      '/transit/metrolink/lines/blue',
    );
    expect(within(affected).getByText('Minor delays')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Share this page' })).toBeTruthy();
    expect(screen.getByText(/Open Government Licence/)).toBeTruthy();
  });

  it("links a notice's stops, and never a non-web link", async () => {
    vi.mocked(api.getTransitSituation).mockResolvedValue(LIFT_DETAIL);
    renderWithMantine(await TransitIncidentPage(params('S2')));
    expect(screen.getByRole('link', { name: 'Piccadilly' }).getAttribute('href')).toBe(
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
    expect(screen.getByText("Doesn't change any line's status.")).toBeTruthy();
    expect(screen.queryByRole('link', { name: /More details/ })).toBeNull();
  });

  it('says when an ended disruption stopped being listed', async () => {
    vi.mocked(api.getTransitSituation).mockResolvedValue({
      ...DELAY_DETAIL,
      state: 'ended',
      endedAt: '2026-10-14T11:30:00Z',
    });
    renderWithMantine(await TransitIncidentPage(params('S1')));
    expect(screen.getByText('Ended')).toBeTruthy();
    expect(screen.getByText(/^No longer listed by the source since /)).toBeTruthy();
    expect(screen.queryByRole('list', { name: 'Lines affected' })).toBeNull();
    // An ended one shows no status chip.
    expect(screen.queryByText('Minor delays')).toBeNull();
  });

  it('says when an upcoming one starts, and when the status is stale', async () => {
    vi.mocked(api.getTransitSituation).mockResolvedValue({
      ...DELAY_DETAIL,
      state: 'upcoming',
      starts: '2026-10-18T05:00:00Z',
      status: { state: 'unavailable', asOf: '2026-10-14T08:00:00Z' },
    });
    renderWithMantine(await TransitIncidentPage(params('S1')));
    expect(screen.getByText('Upcoming')).toBeTruthy();
    expect(screen.getByText(/^Not in effect yet: starts /)).toBeTruthy();
    expect(screen.getByText('Status unavailable. Last read at 09:00.')).toBeTruthy();
  });

  it('is not found for an unknown disruption or a malformed network', async () => {
    vi.mocked(api.getTransitSituation).mockRejectedValue(new ApiNotFoundError('404'));
    await expect(TransitIncidentPage(params('NOPE'))).rejects.toThrow('NEXT_NOT_FOUND');
    await expect(TransitIncidentPage(params('S1', 'Metro Link'))).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('titles the page with the summary, unindexed, with its own preview card', async () => {
    const meta = await generateMetadata(params('S1'));
    expect(meta.title).toBe(DELAY_DETAIL.summary);
    expect(meta.description).toBe('Disruption on Blue Line, Manchester Metrolink.');
    expect(meta.robots).toMatchObject({ index: false });
    expect(JSON.stringify(meta.openGraph)).toContain('/transit/metrolink/incidents/S1/card.png?v=');
  });
});
