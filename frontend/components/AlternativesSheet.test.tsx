import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AlternativesSheet, type AlternativesMode } from './AlternativesSheet';
import { breakResponse, replanSuggestion, suggestionSet, trainSuggestionLeg } from '@/test/replanFixtures';

const refreshMock = vi.fn();
const router = { push: vi.fn(), refresh: refreshMock };
vi.mock('next/navigation', () => ({
  useRouter: () => router,
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

type Handler = (init: RequestInit | undefined) => Response | Promise<Response>;

/** A fetch mock answering by method and path. */
function mockFetch(routes: Record<string, Handler>) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`;
    const handler = routes[key];
    if (!handler) throw new Error(`unexpected fetch ${key}`);
    return handler(init);
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

function renderSheet(mode: AlternativesMode = { kind: 'break', breakId: 5 }, onClose = vi.fn()) {
  renderWithMantine(
    <AlternativesSheet
      journeyId={42}
      mode={mode}
      opened
      onClose={onClose}
      fallbackTitle="Connection at York missed"
      canMute
    />,
  );
  return onClose;
}

const BREAK_URL = 'GET /api/Journeys/42/breaks/current';
const ACCEPT_URL = 'POST /api/Journeys/42/replan/accept';

beforeEach(() => {
  refreshMock.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe('AlternativesSheet', () => {
  it('says it is loading, then shows the cards and more options', async () => {
    let resolve: (response: Response) => void = () => {};
    mockFetch({ [BREAK_URL]: () => new Promise<Response>((r) => (resolve = r)) });
    renderSheet();
    expect(screen.getByText('Loading trains')).toBeInTheDocument();
    await act(async () => resolve(json(breakResponse())));

    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Connection at York missed' })).toBeInTheDocument();
    expect(screen.getByText('3 other ways to Edinburgh. First arrives 12:10.')).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /^Take this one/ })).toHaveLength(2);
    const first = screen.getByText('10:04 York to Edinburgh, direct').closest('[data-suggestion]');
    expect(first).not.toBeNull();
    const card = within(first as HTMLElement);
    expect(card.getByText('Recommended')).toBeInTheDocument();
    expect(card.getByText('Same operator')).toBeInTheDocument();
    expect(card.getByText('Usually covered by your ticket')).toBeInTheDocument();
    expect(card.getByText('+12 min')).toBeInTheDocument();
    expect(card.getByText('Arrives 12 min after your booked time')).toBeInTheDocument();
    expect(card.getByText('Departs 10:04, arrives 12:31')).toBeInTheDocument();

    // The rest are behind "More options".
    expect(screen.queryByText('10:20 York to Edinburgh, direct')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'More options (1)' }));
    expect(await screen.findByText('10:20 York to Edinburgh, direct')).toBeInTheDocument();

    expect(screen.getByText('Your booked plan stays saved for Delay Repay.')).toBeInTheDocument();
  });

  it('opens with the push source when the deep link opened it', async () => {
    const fetchMock = mockFetch({ [`${BREAK_URL}?src=push`]: () => json(breakResponse()) });
    renderSheet({ kind: 'break', breakId: 5, fromPush: true });
    await screen.findByText('10:04 York to Edinburgh, direct');
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/breaks/current?src=push', { method: 'GET' });
  });

  it('shows stale suggestions dimmed while it updates, then swaps them in place', async () => {
    let resolvePreview: (response: Response) => void = () => {};
    const fetchMock = mockFetch({
      [BREAK_URL]: () => json(breakResponse({ stale: true })),
      'POST /api/Journeys/42/replan/preview': () => new Promise<Response>((r) => (resolvePreview = r)),
    });
    renderSheet();
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
    expect(screen.getByText('Updating')).toBeInTheDocument();
    const list = document.querySelector('[data-suggestions]');
    expect(list).toHaveAttribute('data-updating', 'true');
    expect(list).toHaveAttribute('aria-busy', 'true');
    expect(screen.getAllByRole('button', { name: /^Take this one/ })[0]).toBeDisabled();
    const previewCall = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/replan/preview'));
    expect(previewCall?.[1]?.body).toBe(JSON.stringify({ breakId: 5 }));

    const fresh = suggestionSet({
      fingerprint: 'fp-2',
      suggestions: [
        replanSuggestion({
          id: 's9',
          departure: '2026-09-17T09:30:00Z',
          arrival: '2026-09-17T12:00:00Z',
          legs: [trainSuggestionLeg({ departure: '2026-09-17T09:30:00Z' })],
        }),
      ],
    });
    await act(async () => resolvePreview(json(breakResponse({ suggestions: fresh }))));

    expect(await screen.findByText('10:30 York to Edinburgh, direct')).toBeInTheDocument();
    expect(screen.queryByText('10:04 York to Edinburgh, direct')).not.toBeInTheDocument();
    expect(screen.queryByText('Updating')).not.toBeInTheDocument();
    expect(document.querySelector('[data-suggestions]')).not.toHaveAttribute('data-updating');
  });

  it('plans fresh suggestions when none are stored', async () => {
    mockFetch({
      [BREAK_URL]: () => json(breakResponse({ suggestions: null })),
      'POST /api/Journeys/42/replan/preview': () => json(breakResponse()),
    });
    renderSheet();
    expect(await screen.findByText('10:04 York to Edinburgh, direct')).toBeInTheDocument();
  });

  it('takes a suggestion, offers Undo for 30 seconds, then refreshes', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const fetchMock = mockFetch({
      [BREAK_URL]: () => json(breakResponse()),
      [ACCEPT_URL]: () => json({ journeyId: 42, revisionNo: 4, newLegIds: [11], undoRevisionNo: 3 }),
    });
    renderSheet();
    fireEvent.click(await screen.findByRole('button', { name: 'Take this one: 10:04 York to Edinburgh, direct' }));

    expect(await screen.findByText('Journey updated. Booked plan saved.')).toBeInTheDocument();
    const call = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/replan/accept'));
    expect(call?.[1]?.body).toBe(JSON.stringify({ breakId: 5, suggestionId: 's1', fingerprint: 'fp-1' }));
    const headers = call?.[1]?.headers as Record<string, string>;
    expect(headers['Idempotency-Key']).toMatch(/^[0-9a-f-]{36}$/);
    expect(screen.getByRole('button', { name: 'Undo' })).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeInTheDocument();
    expect(refreshMock).toHaveBeenCalledTimes(1);
  });

  it('undoes a change by reverting to the earlier plan', async () => {
    const onClose = vi.fn();
    const fetchMock = mockFetch({
      [BREAK_URL]: () => json(breakResponse()),
      [ACCEPT_URL]: () => json({ journeyId: 42, revisionNo: 4, newLegIds: [11], undoRevisionNo: 3 }),
      'POST /api/Journeys/42/revisions/3/revert': () =>
        json({ journeyId: 42, revisionNo: 5, newLegIds: [7], undoRevisionNo: null }),
    });
    renderSheet({ kind: 'break', breakId: 5 }, onClose);
    fireEvent.click(await screen.findByRole('button', { name: 'Take this one: 10:04 York to Edinburgh, direct' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Undo' }));

    expect(await screen.findByText('Your earlier plan is back.')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith('/api/Journeys/42/revisions/3/revert', { method: 'POST' });
    fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(onClose).toHaveBeenCalled();
    expect(refreshMock).toHaveBeenCalledTimes(1);
  });

  it('swaps in the latest options when the chosen train no longer works', async () => {
    const latest = suggestionSet({
      fingerprint: 'fp-3',
      suggestions: [
        replanSuggestion({
          id: 's4',
          departure: '2026-09-17T09:40:00Z',
          legs: [trainSuggestionLeg({ departure: '2026-09-17T09:40:00Z' })],
        }),
      ],
    });
    mockFetch({
      [BREAK_URL]: () => json(breakResponse()),
      [ACCEPT_URL]: () =>
        json({ error: 'suggestion_stale', message: 'stale', break: breakResponse({ suggestions: latest }) }, 409),
    });
    renderSheet();
    fireEvent.click(await screen.findByRole('button', { name: 'Take this one: 10:04 York to Edinburgh, direct' }));

    expect(await screen.findByText('That train no longer works. Here are the latest options.')).toBeInTheDocument();
    expect(screen.getByText('10:40 York to Edinburgh, direct')).toBeInTheDocument();
    expect(screen.queryByText('10:04 York to Edinburgh, direct')).not.toBeInTheDocument();
  });

  it('says it could not load other trains', async () => {
    mockFetch({ [BREAK_URL]: () => new Response('busy', { status: 503 }) });
    renderSheet();
    expect(await screen.findByText("Couldn't load other trains. Try again.")).toBeInTheDocument();
  });

  it('says it could not update the journey', async () => {
    mockFetch({
      [BREAK_URL]: () => json(breakResponse()),
      [ACCEPT_URL]: () => new Response('oops', { status: 500 }),
    });
    renderSheet();
    fireEvent.click(await screen.findByRole('button', { name: 'Take this one: 10:04 York to Edinburgh, direct' }));
    expect(await screen.findByText("Couldn't update the journey. Try again.")).toBeInTheDocument();
  });

  it('plans from a missed train and warns about the ticket when it was the traveller', async () => {
    const onClose = vi.fn();
    mockFetch({
      'POST /api/Journeys/42/legs/7/missed': () =>
        json(breakResponse({ cause: 'user_missed', fault: 'user', faultNote: 'You missed it by 2 min.' })),
    });
    renderSheet({ kind: 'missed', legId: 7 }, onClose);
    expect(await screen.findByRole('heading', { name: 'You missed the 09:52 to Edinburgh' })).toBeInTheDocument();
    expect(
      screen.getByText('Your ticket may not be valid on a different train. Check before you board.'),
    ).toBeInTheDocument();
    expect(screen.getByText('You missed it by 2 min.')).toBeInTheDocument();
    // The missed train opened a break: closing shows it on the page.
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalled();
    expect(refreshMock).toHaveBeenCalledTimes(1);
  });

  it('replaces the leg when taking a free choice', async () => {
    const fetchMock = mockFetch({
      'GET /api/Journeys/42/legs/7/alternatives?cause=choice': () => json(suggestionSet({ hint: 'Add your ticket.' })),
      'POST /api/Journeys/42/legs/7/replace': () =>
        json({ journeyId: 42, revisionNo: 2, newLegIds: [12], undoRevisionNo: 1 }),
    });
    renderSheet({ kind: 'alternatives', legId: 7, cause: 'choice' });
    expect(await screen.findByRole('heading', { name: 'Other ways to Edinburgh' })).toBeInTheDocument();
    expect(screen.getByText('Add your ticket.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Take this one: 10:04 York to Edinburgh, direct' }));
    expect(await screen.findByText('Journey updated. Booked plan saved.')).toBeInTheDocument();
    const call = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/replace'));
    expect(JSON.parse(String(call?.[1]?.body))).toEqual({
      cause: 'choice',
      legs: [{ trainUid: 'L12345', serviceDate: '2026-09-17', originCrs: 'YRK', destinationCrs: 'EDB' }],
      throughLegId: 7,
    });
  });

  it('mutes suggestions for the journey', async () => {
    const fetchMock = mockFetch({
      [BREAK_URL]: () => json(breakResponse()),
      'PUT /api/Journeys/42/suggestions-muted': () => new Response(null, { status: 204 }),
    });
    renderSheet();
    fireEvent.click(await screen.findByRole('button', { name: 'Stop suggestions for this journey' }));
    expect(await screen.findByText('Suggestions muted for this journey.')).toBeInTheDocument();
    const call = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/suggestions-muted'));
    expect(call?.[1]?.body).toBe(JSON.stringify({ muted: true }));
  });

  it('explains a ticket fit on tap', async () => {
    mockFetch({ [BREAK_URL]: () => json(breakResponse()) });
    renderSheet();
    fireEvent.click(await screen.findByRole('button', { name: 'May need a new ticket: why' }));
    await waitFor(() => expect(screen.getByText('Your Advance ticket is only valid on LNER.')).toBeInTheDocument());
  });
});
