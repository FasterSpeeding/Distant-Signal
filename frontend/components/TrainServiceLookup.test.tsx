import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainServiceLookup } from './TrainServiceLookup';
import { nowInLondon } from '@/lib/londonWallClock';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  // `null`, as outside the app router: the lookup falls back to its props.
  useSearchParams: () => null,
}));

const ROWS = [
  {
    uid: 'C10001',
    scheduled: '06:10',
    stationCrs: 'KGX',
    originCrs: 'KGX',
    originName: 'London Kings Cross',
    destinationCrs: 'EDB',
    destinationName: 'Edinburgh',
    destinationArrival: '10:30',
    operator: 'GR',
    identity: '1S00',
    serviceDate: '2026-10-30',
  },
  {
    uid: 'P20002',
    scheduled: '07:45',
    stationCrs: 'GLC',
    originCrs: 'GLC',
    originName: 'Glasgow Central',
    destinationCrs: 'AYR',
    destinationName: 'Ayr',
    destinationArrival: '08:40',
    operator: 'SR',
    identity: '1S00',
    serviceDate: '2026-10-30',
  },
];

function stubFetch(response: () => Response) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/trains/by-headcode')) return response();
    throw new Error(`unexpected fetch for ${url}`);
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

function lookUp(code: string) {
  fireEvent.change(screen.getByRole('textbox', { name: 'Headcode or UID' }), { target: { value: code } });
  fireEvent.click(screen.getByRole('button', { name: code.length === 6 ? 'Open train' : 'Find' }));
}

describe('TrainServiceLookup', () => {
  beforeEach(() => {
    pushMock.mockClear();
    window.history.replaceState(null, '', '/trains');
  });

  afterEach(() => {
    window.history.replaceState(null, '', '/');
  });

  it('lists every train running under a headcode, each linked to its page, and puts it in the URL', async () => {
    const fetchMock = stubFetch(
      () => new Response(JSON.stringify({ results: ROWS, serviceDate: '2026-10-30' }), { status: 200 }),
    );
    renderWithMantine(<TrainServiceLookup operatorNames={{ GR: 'LNER', SR: 'ScotRail' }} />);
    expect(screen.getByText("Enter a headcode or UID to see that train's timetable.")).toBeInTheDocument();

    lookUp('1s00');

    const today = nowInLondon().format('YYYY-MM-DD');
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe(`/api/trains/by-headcode?headcode=1S00&date=${today}`);
    const edinburgh = await screen.findByRole('link', { name: /^06:10 to Edinburgh\b/ });
    expect(edinburgh).toHaveAttribute('href', '/train/C10001/2026-10-30');
    expect(edinburgh.closest('li')).toHaveTextContent('Starts here · LNER (GR) · UID C10001');
    expect(screen.getByRole('link', { name: /^07:45 to Ayr\b/ })).toHaveAttribute('href', '/train/P20002/2026-10-30');
    expect(screen.getByText(/^2 trains run as 1S00\./)).toBeInTheDocument();
    expect(window.location.search).toBe('?headcode=1S00');
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('opens the train page for a UID, on the date picked or today', () => {
    stubFetch(() => new Response('{}', { status: 500 }));
    renderWithMantine(<TrainServiceLookup initialDate="2026-10-31" />);
    lookUp('w12345');
    expect(pushMock).toHaveBeenCalledWith('/train/W12345/2026-10-31');
  });

  it('looks up a headcode from the URL on arrival', async () => {
    stubFetch(() => new Response(JSON.stringify({ results: [ROWS[0]] }), { status: 200 }));
    renderWithMantine(<TrainServiceLookup initialCode="1S00" initialDate="2026-10-30" />);
    expect(await screen.findByText(/^1 train runs as 1S00\./)).toBeInTheDocument();
  });

  it('says when no train runs as that headcode, and when the timetable is missing', async () => {
    stubFetch(() => new Response(JSON.stringify({ results: [] }), { status: 200 }));
    const { unmount } = renderWithMantine(<TrainServiceLookup />);
    lookUp('9Z99');
    expect(await screen.findByText('No train runs as 9Z99 on that day.')).toBeInTheDocument();
    unmount();

    stubFetch(() => new Response('not found', { status: 404 }));
    renderWithMantine(<TrainServiceLookup initialDate="2026-11-30" />);
    lookUp('9Z99');
    expect(await screen.findByText("Scheduled timetable data for 2026-11-30 isn't available yet.")).toBeInTheDocument();
  });

  it('shows the failure, and asks for a headcode or UID rather than searching junk', async () => {
    const fetchMock = stubFetch(() => new Response('boom', { status: 500 }));
    renderWithMantine(<TrainServiceLookup />);
    lookUp('12');
    expect(screen.getByText('Enter a headcode like 1S00 or a UID like W12345')).toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();

    lookUp('1S00');
    await waitFor(() =>
      expect(screen.getByText("Couldn't search for trains right now. Try again.")).toBeInTheDocument(),
    );
  });
});
