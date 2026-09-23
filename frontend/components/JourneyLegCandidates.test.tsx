import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyLegCandidates } from './JourneyLegCandidates';

// A BTH -> SWI leg riding two Bristol -> London Paddington services: the
// leg's own two ends (`legOriginCrs`/`legDestinationCrs`) are neither of
// the train's (`originCrs`/`destinationCrs`), which is exactly the shape
// that made every row read identically before the 2026-09-22 review's C4.
const CANDIDATES_FIXTURE = {
  results: [
    {
      uid: 'C11052',
      scheduled: '10:32',
      destinationCrs: 'PAD',
      originCrs: 'BRI',
      destinationArrival: '12:05',
      legOriginCrs: 'BTH',
      legDestinationCrs: 'SWI',
      legDestinationArrival: '11:08',
      legDestinationArrivalDayOffset: 0,
    },
    {
      uid: 'C11099',
      scheduled: '11:02',
      destinationCrs: 'PAD',
      originCrs: 'BRI',
      destinationArrival: '12:35',
      legOriginCrs: 'BTH',
      legDestinationCrs: 'SWI',
      legDestinationArrival: '11:38',
      legDestinationArrivalDayOffset: 0,
    },
  ],
  nextCursor: null,
};

/** Routes a mocked `fetch` by URL/method, the same shape
 * `TrackThisTrainButton.test.tsx`'s own `mockFetchByUrl` helper uses: the
 * candidates GET and the train-pick POST are configured independently so a
 * test can make either one fail/succeed without the other. */
function mockFetchByUrl(
  options: {
    candidates?: () => Response | Promise<Response>;
    train?: () => Response | Promise<Response>;
  } = {},
) {
  const {
    candidates = () => new Response(JSON.stringify(CANDIDATES_FIXTURE), { status: 200 }),
    train = () => new Response(JSON.stringify({ ok: true }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (/\/api\/Journeys\/\d+\/legs\/\d+\/train$/.test(url) && init?.method === 'POST') {
      return Promise.resolve(train());
    }
    if (/\/api\/Journeys\/\d+\/legs\/\d+\/candidates$/.test(url)) {
      return Promise.resolve(candidates());
    }
    throw new Error(`unexpected fetch for ${url}`);
  });
}

describe('JourneyLegCandidates', () => {
  const onPicked = vi.fn();

  beforeEach(() => {
    onPicked.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows a loading state while the candidates request is in flight', async () => {
    let resolveCandidates: (value: Response) => void = () => {};
    const pending = new Promise<Response>((resolve) => {
      resolveCandidates = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn(() => pending),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(screen.getByText('Searching for candidate trains…')).toBeInTheDocument();

    // Let the pending fetch settle within `act` so no state update leaks
    // past the end of the test.
    resolveCandidates(new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 }));
    await screen.findByText(/No scheduled trains match this window\./);
  });

  it('leads each row with the traveller\'s own leg times, not the train\'s route', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(await screen.findByText('dep. BTH 10:32 → arr. SWI 11:08')).toBeInTheDocument();
    expect(screen.getByText('dep. BTH 11:02 → arr. SWI 11:38')).toBeInTheDocument();
    // The train's own identity and route survive as dimmed secondary
    // text -- useful context, no longer the whole row.
    expect(screen.getAllByText('Train C11052 · BRI → PAD')).toHaveLength(1);
  });

  // Review §2.5/M15 -- the list's own intro line, added independently of
  // the per-row leg-times rewrite above (both land on this component).
  it('names the match count and says the pick can still be changed', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(
      await screen.findByText("2 trains match your search — pick the one you'll be on. You can change it later."),
    ).toBeInTheDocument();
  });

  it('singularizes the match count for exactly one candidate', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        candidates: () =>
          new Response(JSON.stringify({ results: [CANDIDATES_FIXTURE.results[0]], nextCursor: null }), {
            status: 200,
          }),
      }),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(
      await screen.findByText("1 train matches your search — pick the one you'll be on. You can change it later."),
    ).toBeInTheDocument();
  });

  it('gives every "Track this train" button its own accessible name', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    // The pre-fix state: two buttons, both named exactly "Track this
    // train", indistinguishable in a screen reader's control list.
    expect(await screen.findAllByRole('button', { name: /^Track this train/ })).toHaveLength(2);
    expect(
      screen.getByRole('button', { name: 'Track this train — dep. BTH 10:32 → arr. SWI 11:08' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Track this train — dep. BTH 11:02 → arr. SWI 11:38' }),
    ).toBeInTheDocument();
    // Label-in-Name (WCAG 2.5.3): the accessible name still CONTAINS the
    // visible label.
    expect(screen.getAllByText('Track this train')).toHaveLength(2);
  });

  it('offers a per-row "View live status" link, as the /trains result row does', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    const link = await screen.findByRole('link', {
      name: 'View live status for the dep. BTH 10:32 → arr. SWI 11:08',
    });
    expect(link).toHaveAttribute('href', '/train/C11052/2026-09-22');
  });

  it('omits the arrival rather than guessing when the schedule has none for the leg destination', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        candidates: () =>
          new Response(
            JSON.stringify({
              results: [
                {
                  ...CANDIDATES_FIXTURE.results[0],
                  legDestinationArrival: null,
                },
              ],
              nextCursor: null,
            }),
            { status: 200 },
          ),
      }),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(await screen.findByText('dep. BTH 10:32')).toBeInTheDocument();
    // Never the train's terminus arrival standing in for the leg's own.
    expect(screen.queryByText(/12:05/)).not.toBeInTheDocument();
    expect(screen.queryByText(/arr\./)).not.toBeInTheDocument();
  });

  it('says so when the leg arrives the next day', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        candidates: () =>
          new Response(
            JSON.stringify({
              results: [
                {
                  ...CANDIDATES_FIXTURE.results[0],
                  scheduled: '23:40',
                  legDestinationArrival: '02:15',
                  legDestinationArrivalDayOffset: 1,
                },
              ],
              nextCursor: null,
            }),
            { status: 200 },
          ),
      }),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(
      await screen.findByText('dep. BTH 23:40 → arr. SWI 02:15 (next day)'),
    ).toBeInTheDocument();
  });

  it('POSTs the picked train and calls onPicked on success', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    const buttons = await screen.findAllByRole('button', { name: /^Track this train/ });
    fireEvent.click(buttons[0]);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Journeys/1/legs/2/train',
        expect.objectContaining({
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ trainUid: 'C11052', serviceDate: '2026-09-22' }),
        }),
      );
    });
    await waitFor(() => expect(onPicked).toHaveBeenCalledTimes(1));
  });

  it('shows a pick error and does not call onPicked when the train POST fails', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({ train: () => new Response('boom', { status: 500 }) }),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    const buttons = await screen.findAllByRole('button', { name: /^Track this train/ });
    fireEvent.click(buttons[0]);

    expect(await screen.findByText("Couldn't track that train. Try again.")).toBeInTheDocument();
    expect(onPicked).not.toHaveBeenCalled();
  });

  it('renders the "Search manually" fallback link when there are no candidates', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({ candidates: () => new Response(JSON.stringify({ results: [], nextCursor: null }), { status: 200 }) }),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    const link = await screen.findByRole('link', { name: 'Search manually' });
    expect(link).toHaveAttribute('href', '/track');
    expect(screen.getByText(/No scheduled trains match this window\./)).toBeInTheDocument();
  });

  it('renders an error Alert when the candidates fetch rejects', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new Error('network blip'))),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(await screen.findByText('Search failed')).toBeInTheDocument();
    expect(
      screen.getByText("Couldn't load candidate trains right now. Try again."),
    ).toBeInTheDocument();
  });

  it('renders an error Alert when the candidates response is not ok', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response('boom', { status: 500 }))),
    );
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(await screen.findByText('Search failed')).toBeInTheDocument();
  });

  // Regression: EUS-MKC is a high-frequency corridor with two operators'
  // services interleaved by departure time. The backend's default 50-row
  // page can genuinely fill up on a moderately wide window, and this
  // component used to throw `nextCursor` away entirely -- no "Load more",
  // no indication more results existed, so whichever operator's services
  // sorted past the cutoff silently vanished. These tests pin the fix:
  // `TrainSearchForm.test.tsx`'s own "Load more" tests are the pattern
  // mirrored here.
  describe('pagination', () => {
    const PAGE_ONE = [CANDIDATES_FIXTURE.results[0]];
    const PAGE_TWO = [CANDIDATES_FIXTURE.results[1]];

    function candidatesFetchMock(
      options: {
        page1?: () => Response | Promise<Response>;
        page2?: (url: string) => Response | Promise<Response>;
      } = {},
    ) {
      const {
        page1 = () => new Response(JSON.stringify({ results: PAGE_ONE, nextCursor: 'CURSOR1' }), { status: 200 }),
        page2 = () => new Response(JSON.stringify({ results: PAGE_TWO, nextCursor: null }), { status: 200 }),
      } = options;
      return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input);
        if (/\/api\/Journeys\/\d+\/legs\/\d+\/train$/.test(url) && init?.method === 'POST') {
          return Promise.resolve(new Response(JSON.stringify({ ok: true }), { status: 200 }));
        }
        if (/\/api\/Journeys\/\d+\/legs\/\d+\/candidates\?/.test(url)) {
          return Promise.resolve(page2(url));
        }
        if (/\/api\/Journeys\/\d+\/legs\/\d+\/candidates$/.test(url)) {
          return Promise.resolve(page1());
        }
        throw new Error(`unexpected fetch for ${url}`);
      });
    }

    it('does not offer Load more when the response has no nextCursor', async () => {
      vi.stubGlobal('fetch', mockFetchByUrl());
      renderWithMantine(
        <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
      );

      await screen.findByText('dep. BTH 10:32 → arr. SWI 11:08');
      expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
    });

    it('offers Load more when the first page carries a nextCursor', async () => {
      vi.stubGlobal('fetch', candidatesFetchMock());
      renderWithMantine(
        <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
      );

      expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
    });

    it('appends the second page to the first rather than replacing it, sending after=', async () => {
      const fetchMock = candidatesFetchMock();
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(
        <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
      );

      await screen.findByText('dep. BTH 10:32 → arr. SWI 11:08');
      fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

      expect(await screen.findByText('dep. BTH 11:02 → arr. SWI 11:38')).toBeInTheDocument();
      expect(
        screen.getByText('dep. BTH 10:32 → arr. SWI 11:08'),
        'page 1 must still be on screen -- Load more appends, it does not replace',
      ).toBeInTheDocument();

      const candidateCalls = fetchMock.mock.calls
        .map((call) => String(call[0]))
        .filter((url) => url.includes('/candidates'));
      expect(candidateCalls).toEqual([
        '/api/Journeys/1/legs/2/candidates',
        '/api/Journeys/1/legs/2/candidates?after=CURSOR1',
      ]);

      // The control disappears once nextCursor comes back null.
      await waitFor(() =>
        expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument(),
      );
      expect(
        screen.getByText("You've reached the end — no more candidate trains match this window."),
      ).toBeInTheDocument();
    });

    it('updates the match count to include appended rows', async () => {
      vi.stubGlobal('fetch', candidatesFetchMock());
      renderWithMantine(
        <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
      );

      await screen.findByText(/1 train matches your search/);
      fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

      expect(await screen.findByText(/2 trains match your search/)).toBeInTheDocument();
    });

    it('reports a failed Load more and keeps the cursor for a retry, without losing page 1', async () => {
      vi.stubGlobal(
        'fetch',
        candidatesFetchMock({ page2: () => new Response('boom', { status: 500 }) }),
      );
      renderWithMantine(
        <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
      );

      fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

      expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
      expect(screen.getByText('dep. BTH 10:32 → arr. SWI 11:08')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    });

    it('does not double-fetch page 2 when Load more is clicked rapidly', async () => {
      let resolvePageTwo!: (response: Response) => void;
      const pageTwo = new Promise<Response>((resolve) => {
        resolvePageTwo = resolve;
      });
      const fetchMock = candidatesFetchMock({ page2: () => pageTwo });
      vi.stubGlobal('fetch', fetchMock);
      renderWithMantine(
        <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
      );

      const button = await screen.findByRole('button', { name: 'Load more' });
      fireEvent.click(button);
      fireEvent.click(button);
      fireEvent.click(button);

      resolvePageTwo(new Response(JSON.stringify({ results: PAGE_TWO, nextCursor: null }), { status: 200 }));
      await screen.findByText('dep. BTH 11:02 → arr. SWI 11:38');

      const candidateCalls = fetchMock.mock.calls
        .map((call) => String(call[0]))
        .filter((url) => url.includes('after=CURSOR1'));
      expect(candidateCalls).toHaveLength(1);
    });
  });
});
