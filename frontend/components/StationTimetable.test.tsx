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

const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', stationCrs: 'RDG', originCrs: 'PAD', destinationCrs: 'BRI' },
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
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/trains/search?station=RDG',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
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

  it('shows Load more when nextCursor is non-null, and none when it is null', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE, null), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);
    fireEvent.click(expand());
    await screen.findByText('08:22 · PAD → RDG → BRI');
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
    // The button doesn't just vanish -- the list says it is complete.
    expect(screen.getByText("You've reached the end — no more scheduled departures today.")).toBeInTheDocument();
  });

  it('says the end has been reached once the last page is in, rather than just dropping the button', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) =>
      String(input).includes('after=CURSOR1')
        ? Promise.resolve(new Response(searchBody(PAGE_TWO, null), { status: 200 }))
        : Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 })),
    );
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    // While a next page exists, the end must not be claimed.
    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(
      await screen.findByText("You've reached the end — no more scheduled departures today."),
    ).toBeInTheDocument();
  });

  it('does not claim the end of results when a "Load more" page fails -- it reports the failure and keeps the retry', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) =>
      String(input).includes('after=CURSOR1')
        ? Promise.resolve(new Response('boom', { status: 500 }))
        : Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 })),
    );
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    // The rows already on screen survive a failed next page.
    expect(screen.getByText('08:22 · PAD → RDG → BRI')).toBeInTheDocument();
  });

  it('retrying a failed page really does page on, clearing the error and ending the list', async () => {
    let afterCalls = 0;
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      if (!String(input).includes('after=CURSOR1')) {
        return Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }));
      }
      afterCalls += 1;
      return afterCalls === 1
        ? Promise.resolve(new Response('boom', { status: 500 }))
        : Promise.resolve(new Response(searchBody(PAGE_TWO, null), { status: 200 }));
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    await screen.findByText("Couldn't load more results. Try again.");

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument();
    expect(screen.getByText("You've reached the end — no more scheduled departures today.")).toBeInTheDocument();
  });

  it('collapsing mid-"Load more" leaves a working button on re-expand, not one stuck spinning', async () => {
    // The in-flight page is ABORTED by the collapse, and an aborted request
    // deliberately skips its own cleanup -- so nothing else clears the
    // "loading more" flag. Without an explicit reset the re-expanded panel
    // renders a permanently disabled, permanently spinning button: the exact
    // "button that does nothing" this footer exists to eliminate.
    const fetchMock = vi.fn((input: RequestInfo | URL) =>
      String(input).includes('after=CURSOR1')
        ? new Promise<Response>(() => {}) // never resolves; the collapse aborts it
        : Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 })),
    );
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    fireEvent.click(expand()); // collapse, aborting the in-flight page
    fireEvent.click(expand()); // re-expand: a fresh first page

    await screen.findByText('08:22 · PAD → RDG → BRI');
    expect(await screen.findByRole('button', { name: 'Load more' })).toBeEnabled();
  });

  it('a fresh first page clears a previous "Load more" failure', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) =>
      String(input).includes('after=CURSOR1')
        ? Promise.resolve(new Response('boom', { status: 500 }))
        : Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 })),
    );
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    await screen.findByText("Couldn't load more results. Try again.");

    fireEvent.click(expand()); // collapse
    fireEvent.click(expand()); // re-expand

    await screen.findByText('08:22 · PAD → RDG → BRI');
    await waitFor(() =>
      expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument(),
    );
  });

  it('shows neither Load more nor the end-of-results line when the first page came back empty', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody([]), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    await screen.findByText('No scheduled departures found for the rest of today.');
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('Load more fetches with after=<cursor> and station unchanged, and appends rather than replaces', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('after=CURSOR1')) {
        return Promise.resolve(new Response(searchBody(PAGE_TWO, null), { status: 200 }));
      }
      return Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }));
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.getByText('08:22 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.getByText('10:05 · WAT → RDG → EXD')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      '/api/trains/search?station=RDG&after=CURSOR1',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument());
  });

  it('collapse then re-expand issues a fresh fetch rather than reusing the previous result set', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(searchBody(PAGE_ONE), { status: 200 }))
      .mockResolvedValueOnce(new Response(searchBody(PAGE_TWO), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    await screen.findByText('08:22 · PAD → RDG → BRI');

    fireEvent.click(expand()); // collapse
    fireEvent.click(expand()); // re-expand

    await screen.findByText('11:40 · PAD → RDG → BRI');
    expect(screen.queryByText('08:22 · PAD → RDG → BRI')).not.toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('a stale response from a superseded expand does not corrupt state once a newer request resolves', async () => {
    // Two overlapping, manually-resolved fetches: the first (superseded)
    // request resolves *after* the second (current) one, simulating the
    // out-of-order response a slow first request could produce once the
    // user has collapsed and re-expanded. Neither `Response` reacts to the
    // request's `AbortSignal` -- resolving them manually proves the
    // component itself discards the stale response (via its own
    // `signal.aborted` check after `await`), not merely that the network
    // layer happened to reject an aborted fetch.
    let resolveFirst!: (response: Response) => void;
    let resolveSecond!: (response: Response) => void;
    const firstResponse = new Promise<Response>((resolve) => {
      resolveFirst = resolve;
    });
    const secondResponse = new Promise<Response>((resolve) => {
      resolveSecond = resolve;
    });
    const fetchMock = vi.fn().mockReturnValueOnce(firstResponse).mockReturnValueOnce(secondResponse);
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand()); // starts the first (soon-to-be-stale) request
    fireEvent.click(expand()); // collapse
    fireEvent.click(expand()); // re-expand: starts the second, current request

    expect(fetchMock).toHaveBeenCalledTimes(2);

    // Resolve the newer request first, then the stale one out of order.
    resolveSecond(new Response(searchBody(PAGE_TWO), { status: 200 }));
    await screen.findByText('11:40 · PAD → RDG → BRI');

    resolveFirst(new Response(searchBody(PAGE_ONE), { status: 200 }));
    // Give the stale response's promise chain a turn to (not) run its
    // state updates before asserting nothing changed.
    await waitFor(() => expect(screen.getByText('11:40 · PAD → RDG → BRI')).toBeInTheDocument());

    expect(screen.queryByText('08:22 · PAD → RDG → BRI')).not.toBeInTheDocument();
    expect(screen.queryByText('10:05 · WAT → RDG → EXD')).not.toBeInTheDocument();
    expect(screen.queryByText('Loading scheduled departures…')).not.toBeInTheDocument();
  });

  it('shows a disclaimer above the rows once expanded with results', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText(/These are from the scheduled timetable, not live running information/),
    ).toBeInTheDocument();
  });

  it('offers a link to the full /trains search, prefilled with this station, regardless of expand state', () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    expect(screen.getByRole('link', { name: /Search a different day or filter/ })).toHaveAttribute(
      'href',
      '/trains?station=RDG',
    );
  });

  it('notes that trains terminating at this station will not appear, and that no headcode/operator is shown', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    // This copy lives inside the AccordionPanel alongside the disclaimer
    // (Step 3), so -- like the "shows a disclaimer" test above -- it only
    // renders once expanded: with keepMounted={false}, Mantine's Collapse
    // never mounts panel children at all while the section has never been
    // opened (not merely "later" -- there is no un-clicked path to this
    // text), so the section must be expanded first, then awaited per the
    // documented Activity-API mount quirk.
    fireEvent.click(expand());

    expect(
      await screen.findByText(/only departures from this station -- trains that terminate here won't be listed/i),
    ).toBeInTheDocument();
  });
});
