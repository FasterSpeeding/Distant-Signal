import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { StationTimetable } from './StationTimetable';

/** Builds a `GET /public/trains/search` response body -- same envelope
 * shape `TrainSearchForm.test.tsx::searchBody` builds against the same
 * route. */
function searchBody(
  rows: Array<{
    uid: string;
    scheduled: string;
    stationCrs: string;
    originCrs: string | null;
    destinationCrs: string | null;
  }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({
    results: rows.map((row) => ({ destinationArrival: null, destinationArrivalDayOffset: 0, ...row })),
    nextCursor,
  });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', stationCrs: 'RDG', originCrs: 'PAD', destinationCrs: 'BRI' },
  { uid: 'C10002', scheduled: '10:05', stationCrs: 'RDG', originCrs: 'WAT', destinationCrs: 'EXD' },
];

function expand() {
  return screen.getByRole('button', { name: 'Scheduled departures' });
}

describe('StationTimetable', () => {
  it('renders collapsed by default: the control is present, but no fetch happens and no panel content is in the document', () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<StationTimetable crs="RDG" />);

    expect(screen.getByRole('button', { name: 'Scheduled departures' })).toBeInTheDocument();
    expect(screen.queryByText('Loading scheduled departures…')).not.toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('fetches exactly once, to /api/trains/search?station=<CRS> uppercased, on first expand', async () => {
    const fetchMock = vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 })));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="rdg" />);

    fireEvent.click(expand());

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    expect(fetchMock).toHaveBeenCalledWith('/api/trains/search?station=RDG');
  });

  it('shows a loading state between expand and the fetch resolving', async () => {
    const fetchMock = vi.fn(() => new Promise(() => {})); // never resolves
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    await waitFor(() => expect(screen.getByText('Loading scheduled departures…')).toBeInTheDocument());
  });

  it('renders one row per result, with time/origin/destination and a link to the live status page for today', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(await screen.findByText('08:22 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.getByText('10:05 · WAT → RDG → EXD')).toBeInTheDocument();
    const links = screen.getAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a "?" placeholder when origin or destination is unresolved', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() =>
        Promise.resolve(
          new Response(
            searchBody([{ uid: 'C99999', scheduled: '09:00', stationCrs: 'RDG', originCrs: null, destinationCrs: 'BRI' }]),
            { status: 200 },
          ),
        ),
      ),
    );
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(await screen.findByText('09:00 · ? → RDG → BRI')).toBeInTheDocument();
  });

  it('shows the "no matches today" copy for a 200 with an empty results array', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody([]), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(await screen.findByText('No scheduled departures found for the rest of today.')).toBeInTheDocument();
  });

  it('shows the "not available yet" copy on a 404, distinct from the empty-results copy', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response('not found', { status: 404 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText("Today's scheduled timetable data isn't available yet."),
    ).toBeInTheDocument();
    expect(screen.queryByText('No scheduled departures found for the rest of today.')).not.toBeInTheDocument();
  });

  it('shows an error alert on a non-2xx, non-404 response', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response('boom', { status: 500 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText("Couldn't load the scheduled departures right now."),
    ).toBeInTheDocument();
  });

  it('shows an error alert when fetch itself throws', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('network down'))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText("Couldn't load the scheduled departures right now."),
    ).toBeInTheDocument();
  });
});
