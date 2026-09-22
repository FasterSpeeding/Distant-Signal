import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LineTrainsResults } from './LineTrainsResults';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';

vi.mock('@/lib/api');

function entry(overrides: Partial<LineTrainEntry> = {}): LineTrainEntry {
  return {
    uid: 'C12345',
    callingPoints: [
      {
        tiploc: 'WATRLMN',
        kind: 'Origin',
        booked_arrival: null,
        booked_departure: '08:00:00',
        is_half_minute_arrival: false,
        is_half_minute_departure: false,
        day_offset: 0,
      },
    ],
    liveStatus: null,
    ...overrides,
  };
}

describe('LineTrainsResults', () => {
  it('renders the "not available" state on a 404', async () => {
    vi.mocked(api.getLineTrains).mockRejectedValue(new ApiNotFoundError('not found'));
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText('No scheduled train data is available for this line today.')).toBeInTheDocument();
  });

  it('renders the honest outage state on a non-404 failure', async () => {
    vi.mocked(api.getLineTrains).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText("Today's trains aren't available right now.")).toBeInTheDocument();
  });

  it('renders the empty state for a real 200 [] response', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText('No trains are scheduled on this line today.')).toBeInTheDocument();
  });

  it('renders a schedule-only row without a route, with the not-live copy', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([entry()]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText('Scheduled — not live yet')).toBeInTheDocument();
    expect(screen.getByText(/08:00/)).toBeInTheDocument();
  });

  it('renders a live row\'s resolved route and links to /train/{uid}/{date}', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText(/London Waterloo \(WAT\) → Alton \(ALT\)/)).toBeInTheDocument();
    expect(screen.getByText(/4m late/)).toBeInTheDocument();
    const link = screen.getByRole('link', { name: 'View live status' });
    expect(link).toHaveAttribute('href', '/train/C12345/2026-09-22');
  });

  it('sorts rows by their own scheduled time, not response order', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({ uid: 'LATER', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '10:00:00' }] }),
      entry({ uid: 'EARLIER', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '06:00:00' }] }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    const links = screen.getAllByRole('link', { name: 'View live status' });
    expect(links[0]).toHaveAttribute('href', '/train/EARLIER/2026-09-22');
    expect(links[1]).toHaveAttribute('href', '/train/LATER/2026-09-22');
  });

  it('renders "Cancelled" for a cancelled service and suppresses the delay figure', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'cancelled',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText(/Cancelled/)).toBeInTheDocument();
    expect(screen.queryByText(/4m late/)).not.toBeInTheDocument();
  });

  it("displays the schedule-side time, not the live service's origin-departure time", async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        // The live row's `scheduledDeparture` is the whole service's ORIGIN
        // departure time -- deliberately different here from the
        // schedule-side `booked_departure` at this line's own first calling
        // point, as happens for a train that originates off-line. Under the
        // pre-fix code this would have rendered as "08:30" (Europe/London,
        // BST, formatted via `formatTime`) instead of the schedule-side
        // "09:15" the row's own sort key is based on.
        callingPoints: [{ ...entry().callingPoints![0], booked_departure: '09:15:00' }],
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T07:30:00Z',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: null,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText(/09:15/)).toBeInTheDocument();
    expect(screen.queryByText(/08:30/)).not.toBeInTheDocument();
  });
});
