import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JourneyLegCandidates } from './JourneyLegCandidates';

const CANDIDATES_FIXTURE = {
  results: [
    {
      uid: 'C11052',
      scheduled: '10:32',
      destinationCrs: 'PAD',
      originCrs: 'BRI',
      destinationArrival: '12:05',
    },
    {
      uid: 'C11099',
      scheduled: '11:02',
      destinationCrs: 'PAD',
      originCrs: 'BRI',
      destinationArrival: '12:35',
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

  it('renders a "Track this train" button per candidate row', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    expect(await screen.findAllByRole('button', { name: 'Track this train' })).toHaveLength(2);
    expect(screen.getByText('10:32 · BRI → PAD')).toBeInTheDocument();
    expect(screen.getByText('11:02 · BRI → PAD')).toBeInTheDocument();
  });

  it('POSTs the picked train and calls onPicked on success', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(
      <JourneyLegCandidates journeyId={1} legId={2} serviceDate="2026-09-22" onPicked={onPicked} />,
    );

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
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

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
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
});
