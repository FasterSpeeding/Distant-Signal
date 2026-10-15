import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { IncidentSearchForm } from './IncidentSearchForm';
import type { IncidentSearchResponse, LineSummary, Suggestion } from '@/lib/types';

// Same rationale as `TrainSearchForm.test.tsx`'s identical mock: `DatePickerInput`'s
// real popover calendar has no real `<input>` `fireEvent.change` can drive.
// Kept to the same `onChange(string | null)` contract this form actually
// depends on.
vi.mock('@mantine/dates', () => ({
  DatePickerInput: ({
    label,
    value,
    onChange,
  }: {
    label: string;
    value: string | null;
    onChange: (value: string | null) => void;
  }) => (
    <div>
      <label htmlFor={`test-date-${label}`}>{label}</label>
      <input id={`test-date-${label}`} value={value ?? ''} onChange={(event) => onChange(event.target.value || null)} />
    </div>
  ),
}));

const TEST_LINES: LineSummary[] = [
  { id: 'south-western', name: 'South Western Main Line', category: 'main', operators: ['SW'], source: 'catalogue' },
  { id: 'my-custom-line', name: 'My Custom Line', category: 'main', operators: ['SW'], source: 'custom' },
];
const TEST_TOCS: Suggestion[] = [
  { code: 'SW', name: 'South Western Railway' },
  { code: 'VT', name: 'Avanti West Coast' },
];

// The form never navigates: filter changes go to the URL through
// `history.replaceState` (see the URL tests below), so a router call here
// would be the server re-render this page must not trigger.
const pushMock = vi.fn();
const replaceMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, replace: replaceMock }),
  usePathname: () => '/incidents',
  // `null`, as outside the app router: the form falls back to its props.
  useSearchParams: () => null,
}));

const fetchMock = vi.fn();

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockReset();
  pushMock.mockClear();
  replaceMock.mockClear();
  window.history.replaceState(null, '', '/incidents');
});

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState(null, '', '/');
});

function okResponse(body: IncidentSearchResponse) {
  return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) } as Response);
}

function errorResponse() {
  return Promise.resolve({ ok: false, status: 500, json: () => Promise.resolve({}) } as Response);
}

function requestUrl(call: number): URL {
  return new URL(String(fetchMock.mock.calls[call]![0]), 'http://localhost');
}

function lastRequestUrl(): URL {
  return requestUrl(fetchMock.mock.calls.length - 1);
}

/** Waits for the search in flight (the mount search, or the live search a
 * filter change set off) to land: the search box's spinner goes away. */
async function searchSettled() {
  await waitFor(() => expect(fetchMock).toHaveBeenCalled());
  await waitFor(() => expect(screen.queryByTestId('list-filter-pending')).not.toBeInTheDocument());
}

/** Opens the Filters popover, where every filter but the search lives. */
function openFilters() {
  fireEvent.click(screen.getByRole('button', { name: /^Filters/ }));
}

function summary(overrides: Partial<IncidentSearchResponse['results'][number]> = {}) {
  return {
    incidentId: '1',
    summary: 'Signal failure at Woking',
    operators: ['VT'],
    // Empty on every real row: the Knowledgebase feed has no station codes.
    affectedStations: [],
    affectedLines: ['south-western'],
    priority: 3,
    isPlanned: false,
    isCleared: false,
    firstSeenAt: '2026-08-30T09:00:00Z',
    fetchedAt: '2026-08-31T10:15:00Z',
    sourceRemovedAt: null as string | null,
    ...overrides,
  };
}

describe('IncidentSearchForm', () => {
  it('has one search box and a Filters button, and no Search button: filters apply live', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null, total: 0 }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    expect(screen.getByRole('textbox', { name: 'Find an incident' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Filters/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument();
    await searchSettled();
  });

  it('excludes a custom line from the Line dropdown, offering only catalogue lines', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    openFilters();
    // Synchronously, with no `await` in between: an intervening await lets
    // Mantine's floating-ui positioning collapse the just-opened dropdown
    // to `display: none` under jsdom's synthetic layout.
    fireEvent.click(screen.getByRole('combobox', { name: /^Line/ }));
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['South Western Main Line']);
    await searchSettled();
  });

  it('searches the last 30 days on mount, once, without touching the URL', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    const replaceState = vi.spyOn(window.history, 'replaceState');
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await searchSettled();

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const from = requestUrl(0).searchParams.get('from');
    expect(from).not.toBeNull();
    const daysAgo = Math.round((Date.now() - new Date(from as string).getTime()) / (1000 * 60 * 60 * 24));
    expect(daysAgo).toBeGreaterThanOrEqual(29);
    expect(daysAgo).toBeLessThanOrEqual(31);
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(replaceState).not.toHaveBeenCalled();
    replaceState.mockRestore();
  });

  it('searches as the visitor types, sending q, and counts the matches', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null, total: 0 }))
      .mockReturnValue(okResponse({ results: [summary()], nextCursor: null, total: 1 }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await searchSettled();

    const box = screen.getByRole('textbox', { name: 'Find an incident' });
    fireEvent.change(box, { target: { value: 's' } });
    fireEvent.change(box, { target: { value: 'signal' } });
    await screen.findByText('Signal failure at Woking');
    await searchSettled();

    // One request for the settled text, not one per keystroke.
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(lastRequestUrl().searchParams.get('q')).toBe('signal');
    expect(screen.getAllByRole('status').map((el) => el.textContent)).toContain('1 incident matches.');
    fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
    await waitFor(() => expect(lastRequestUrl().searchParams.has('q')).toBe(false));
  });

  it('builds a comma-joined operator query parameter from multiple selected operators', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    openFilters();
    // Both options from one dropdown open, queried synchronously -- see the
    // Line dropdown test above.
    fireEvent.click(screen.getByRole('combobox', { name: /^Operator/ }));
    fireEvent.click(screen.getByRole('option', { name: /SW/ }));
    fireEvent.click(screen.getByRole('option', { name: /VT/ }));

    await waitFor(() => expect(lastRequestUrl().searchParams.get('operator')).toBe('SW,VT'));
    expect(screen.getByTestId('list-filter-count')).toHaveTextContent('2');
  });

  // The Line filter's whole point is answering "which railway was this?",
  // and `affectedStations` can never answer it. A result row shows its
  // `affectedLines` by catalogue NAME, falling back to the raw id for a
  // line the catalogue no longer lists.
  it('labels a result row with its affected lines, by name where the catalogue knows them', async () => {
    fetchMock.mockReturnValue(
      okResponse({
        results: [summary({ incidentId: '1', affectedLines: ['south-western', 'retired-line'] })],
        nextCursor: null,
      }),
    );
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');

    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    expect(list.textContent).toContain('South Western Main Line');
    expect(list.textContent).toContain('retired-line');
    // Plain dimmed text, not a pill per line: a row carries at most two badges.
    expect(within(list).queryAllByTitle('Affected line')).toHaveLength(0);
  });

  it('collapses a long affected-lines list into a "+N more" badge', async () => {
    fetchMock.mockReturnValue(
      okResponse({
        results: [
          summary({
            incidentId: '1',
            affectedLines: ['line-1', 'line-2', 'line-3', 'line-4', 'line-5', 'line-6'],
          }),
        ],
        nextCursor: null,
      }),
    );
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');

    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    expect(list.textContent).toContain('line-4');
    expect(list.textContent).toContain('+2 more');
    expect(list.textContent).not.toContain('line-5');
  });

  // A rolling deploy can serve this bundle against an api that predates
  // `affectedLines` (or `total`). Rendering must degrade, never throw.
  it('renders a row with no affectedLines from a response with no total', async () => {
    const { affectedLines: _dropped, ...withoutLines } = summary({ incidentId: '1' });
    fetchMock.mockReturnValue(okResponse({ results: [withoutLines], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    expect(await screen.findByText('Signal failure at Woking')).toBeTruthy();
  });

  it('sends London-day bounds for custom dates (FE-5)', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await searchSettled();
    openFilters();
    fireEvent.click(screen.getByRole('radio', { name: 'Custom dates' }));
    fireEvent.change(screen.getByLabelText('From'), { target: { value: '2026-05-10' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: '2026-09-15' } });

    // 15 Sep is in BST, so the London day ends at 22:59:59.999Z.
    await waitFor(() => expect(lastRequestUrl().searchParams.get('to')).toBe('2026-09-15T22:59:59.999Z'));
    expect(lastRequestUrl().searchParams.get('from')).toBe('2026-05-09T23:00:00.000Z');
  });

  it('keeps the original filters on a "Load more" request, ignoring a filter change made afterward', async () => {
    let resolveLive!: (response: Response) => void;
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      // The live search the filter change below sets off: held open, so the
      // rows on screen are still page 1 of the first search.
      .mockReturnValueOnce(new Promise<Response>((resolve) => (resolveLive = resolve)))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');
    openFilters();
    fireEvent.change(screen.getByLabelText('Minimum'), { target: { value: '4' } });
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
    expect(requestUrl(2).searchParams.has('priority_min')).toBe(false);
    expect(requestUrl(2).searchParams.get('after')).toBe('cursor-a');
    resolveLive({ ok: true, status: 200, json: () => Promise.resolve({ results: [], nextCursor: null }) } as Response);
    await searchSettled();
  });

  it('"Load more" appends rows rather than replacing them, and disappears once nextCursor is null', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a', total: 2 }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null, total: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');
    expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(1);

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(2));
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
    // The later page's `total: null` doesn't wipe the first page's count.
    openFilters();
    expect(screen.getByRole('button', { name: 'Show 2 incidents' })).toBeInTheDocument();

    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    for (const row of screen.getAllByText('Signal failure at Woking')) {
      expect(list.contains(row)).toBe(true);
    }
  });

  it('picks Ended in the Status filter and writes state=ended to both the request and the URL', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await searchSettled();
    openFilters();
    fireEvent.click(screen.getByRole('radio', { name: 'Ended' }));

    await waitFor(() => expect(lastRequestUrl().searchParams.get('state')).toBe('ended'));
    await waitFor(() => expect(window.location.search).toContain('state=ended'));
    expect(replaceMock).not.toHaveBeenCalled();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('badges each row Active, Ended or Cleared, saying when an ended one was last listed', async () => {
    fetchMock.mockReturnValue(
      okResponse({
        results: [
          summary({ incidentId: 'a', summary: 'Still listed' }),
          summary({ incidentId: 'e', summary: 'Purged overnight', sourceRemovedAt: '2026-10-05T22:55:00+00:00' }),
          summary({ incidentId: 'c', summary: 'Cleared by RDM', isCleared: true }),
        ],
        nextCursor: null,
      }),
    );
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Purged overnight');

    const states = Array.from(document.querySelectorAll('[data-incident-state]')).map((el) =>
      el.getAttribute('data-incident-state'),
    );
    expect(states).toEqual(['active', 'ended', 'cleared']);
    expect(document.querySelector('[data-incident-state="ended"]')).toHaveAttribute(
      'title',
      'No longer listed by the source since 5 Oct 2026, 23:55',
    );
  });

  // Regression guard for the archive being hard-clipped at a fixed height
  // (a `<ScrollArea mah={520}>` whose viewport never scrolled). jsdom does
  // no layout, so this asserts the structure instead: no Mantine scroll
  // viewport and no pinned height between the list and the form's root.
  it('renders the results list in the page flow, with no fixed-height or scroll-container ancestor', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: null }));
    const { container } = renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');

    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    for (let node: HTMLElement | null = list; node !== null && node !== container; node = node.parentElement) {
      expect(node.hasAttribute('data-scrollarea-viewport')).toBe(false);
      expect(node.style.maxHeight).toBe('');
      expect(node.style.height).toBe('');
      expect(node.style.overflow).not.toBe('hidden');
      expect(node.style.overflowY).not.toBe('hidden');
    }
    const header = screen.getByText('Signal failure at Woking').closest('a')?.parentElement as HTMLElement;
    expect(header).toHaveClass('incidentRow__head');
    expect(screen.getByText(/2026/)).toHaveClass('ds-nowrap');
  });

  it('says the end has been reached once the last page is in, rather than just dropping the button', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    expect(
      await screen.findByText("You've reached the end. No more incidents match these filters."),
    ).toBeInTheDocument();
  });

  it('does not claim the end of results when a "Load more" page fails -- it reports the failure and keeps the retry', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(errorResponse());

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();

    fetchMock.mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(2));
    expect(requestUrl(2).searchParams.get('after')).toBe('cursor-a');
    expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument();
  });

  it('reports a "Load more" whose fetch throws the same way it reports a non-2xx', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      // Lazily: a `Promise.reject` built at setup time is unhandled until used.
      .mockImplementationOnce(() => Promise.reject(new Error('network down')));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
  });

  it('discards a "Load more" page that lands after a fresh search has already replaced the results', async () => {
    let resolvePageTwo!: (response: Response) => void;
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(new Promise<Response>((resolve) => (resolvePageTwo = resolve)))
      .mockReturnValueOnce(
        okResponse({
          results: [summary({ incidentId: '9', summary: 'Points failure at Woking' })],
          nextCursor: null,
        }),
      );

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Signal failure at Woking');
    fireEvent.click(screen.getByRole('button', { name: 'Load more' })); // page 2 of search 1
    fireEvent.change(screen.getByRole('textbox', { name: 'Find an incident' }), { target: { value: 'points' } });
    await screen.findByText('Points failure at Woking');

    resolvePageTwo({
      ok: true,
      status: 200,
      json: () =>
        Promise.resolve({
          results: [summary({ incidentId: '2', summary: 'Trespass incident at Woking' })],
          nextCursor: 'cursor-b',
        }),
    } as unknown as Response);

    await waitFor(() =>
      expect(screen.getByText("You've reached the end. No more incidents match these filters.")).toBeInTheDocument(),
    );
    expect(screen.queryByText('Trespass incident at Woking')).not.toBeInTheDocument();
  });

  it('never lets a slow, superseded search land over a newer one', async () => {
    let resolveSlow!: (response: Response) => void;
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(new Promise<Response>((resolve) => (resolveSlow = resolve)))
      .mockReturnValueOnce(okResponse({ results: [summary({ summary: 'Newer search' })], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await searchSettled();
    const box = screen.getByRole('textbox', { name: 'Find an incident' });
    fireEvent.change(box, { target: { value: 'old' } });
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    fireEvent.change(box, { target: { value: 'newer' } });
    await screen.findByText('Newer search');
    resolveSlow({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ results: [summary({ summary: 'Old search' })], nextCursor: null }),
    } as unknown as Response);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByText('Old search')).not.toBeInTheDocument();
    // The superseded request was aborted.
    expect((fetchMock.mock.calls[1]![1] as RequestInit).signal?.aborted).toBe(true);
  });

  it('renders the empty-results message, not a blank screen', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('No incidents match these filters.');
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
  });

  it('renders an error message on a failed search, not a thrown error', async () => {
    fetchMock.mockReturnValue(errorResponse());
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await screen.findByText('Search failed');
    await searchSettled();
  });

  it('pads the operator and line filter clear buttons to the 24px touch-target floor', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(
      <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialOperator="SW" initialLine="south-western" />,
    );
    await searchSettled();
    openFilters();
    expect(screen.getByLabelText('Clear operator filter').className).toContain('iconHitArea24');
    expect(screen.getByLabelText('Clear line filter').className).toContain('iconHitArea24');
  });

  it('Clear filters goes back to the last 30 days and nothing else set', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null, total: 0 }));
    renderWithMantine(
      <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialOperator="SW" initialPeriod="all" />,
    );
    await searchSettled();
    expect(screen.getByTestId('list-filter-count')).toHaveTextContent('2');
    openFilters();
    fireEvent.click(
      within(screen.getByRole('dialog', { name: 'Filters' })).getByRole('button', { name: 'Clear filters' }),
    );
    expect(screen.getByRole('radio', { name: '30 days' })).toBeChecked();
    expect(screen.queryByTestId('list-filter-count')).not.toBeInTheDocument();
    await waitFor(() => expect(lastRequestUrl().searchParams.has('operator')).toBe(false));
  });

  describe('the Period, Type and Status controls', () => {
    it('defaults to the 30-day preset, with the date pickers hidden', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await searchSettled();
      openFilters();
      expect(screen.getByRole('radiogroup', { name: 'Period' })).toBeInTheDocument();
      expect(screen.getByRole('radio', { name: '30 days' })).toBeChecked();
      expect(screen.queryByLabelText('From')).not.toBeInTheDocument();
      expect(screen.getByRole('radiogroup', { name: 'Type' })).toBeInTheDocument();
      expect(screen.getByRole('radiogroup', { name: 'Status' })).toBeInTheDocument();
    });

    it('shows Custom dates as selected when initial filters supply an explicit from date', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialFrom="2026-08-01T00:00:00Z" />);
      await searchSettled();
      openFilters();
      expect(screen.getByRole('radio', { name: 'Custom dates' })).toBeChecked();
      expect(screen.getByLabelText<HTMLInputElement>('From').value).toBe('2026-08-01');
    });
  });

  describe('the Priority range control (review §2.14)', () => {
    it('states the raw-feed caveat once, with accessible Minimum/Maximum fields', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await searchSettled();
      openFilters();
      expect(screen.getByText('Priority range')).toBeInTheDocument();
      expect(screen.getAllByText(/no documented meaning/i)).toHaveLength(1);
      expect(screen.getByLabelText('Minimum')).toBeInTheDocument();
      expect(screen.getByLabelText('Maximum')).toBeInTheDocument();
    });

    it('shows the min/max error once, and does not search an inverted range', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await searchSettled();
      openFilters();
      fireEvent.change(screen.getByLabelText('Maximum'), { target: { value: '5' } });
      await waitFor(() => expect(lastRequestUrl().searchParams.get('priority_max')).toBe('5'));
      const calls = fetchMock.mock.calls.length;
      fireEvent.change(screen.getByLabelText('Minimum'), { target: { value: '10' } });
      expect(screen.getByText('Minimum must not exceed maximum')).toBeInTheDocument();
      await new Promise((resolve) => setTimeout(resolve, 500));
      expect(fetchMock).toHaveBeenCalledTimes(calls);
    });
  });

  // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md:
  // following a result and pressing Back must restore the search. The URL is
  // written with `history.replaceState` (debounced), never `router.replace`,
  // which re-ran the page's server render (refetching every line and
  // operator) on each change.
  describe('URL state', () => {
    it('writes the filters to the URL with replaceState, never the router', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await searchSettled();
      openFilters();
      fireEvent.click(screen.getByRole('radio', { name: 'All time' }));
      fireEvent.change(screen.getByLabelText('Minimum'), { target: { value: '2' } });
      fireEvent.change(screen.getByRole('textbox', { name: 'Find an incident' }), { target: { value: 'signal' } });

      await waitFor(() => expect(window.location.search).toBe('?q=signal&priority_min=2&period=all'));
      expect(replaceMock).not.toHaveBeenCalled();
      expect(pushMock).not.toHaveBeenCalled();
      // `period` is a URL-only marker: the api never sees it.
      await waitFor(() => expect(lastRequestUrl().searchParams.get('priority_min')).toBe('2'));
      expect(lastRequestUrl().searchParams.has('period')).toBe(false);
      expect(lastRequestUrl().searchParams.has('from')).toBe(false);
    });

    it('writes a preset as period, and the 30-day default as nothing', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialOperator="SW" />);
      await searchSettled();
      openFilters();
      fireEvent.click(screen.getByRole('radio', { name: '7 days' }));
      await waitFor(() => expect(window.location.search).toBe('?operator=SW&period=7d'));
      fireEvent.click(screen.getByRole('radio', { name: '30 days' }));
      await waitFor(() => expect(window.location.search).toBe('?operator=SW'));
    });

    it('keeps params it does not own', async () => {
      window.history.replaceState(null, '', '/incidents?utm=x');
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await searchSettled();
      fireEvent.change(screen.getByRole('textbox', { name: 'Find an incident' }), { target: { value: 'flood' } });
      await waitFor(() => expect(window.location.search).toBe('?utm=x&q=flood'));
    });

    it('restores the search, Type, Status and priority range from initial props, and searches with them once', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm
          lines={TEST_LINES}
          tocs={TEST_TOCS}
          initialQuery="signal"
          initialFrom="2026-08-01T00:00:00Z"
          initialPlanned="true"
          initialCleared="false"
          initialPriorityMin="2"
          initialPriorityMax="8"
        />,
      );
      await searchSettled();
      expect(fetchMock).toHaveBeenCalledTimes(1);
      const url = requestUrl(0);
      expect(url.searchParams.get('q')).toBe('signal');
      // 00:00Z on 1 Aug is 01:00 London, so the London day starts at 23:00Z
      // the day before (FE-5).
      expect(url.searchParams.get('from')).toBe('2026-07-31T23:00:00.000Z');
      expect(url.searchParams.get('planned')).toBe('true');
      // The legacy `cleared=false` restores Active, sent as `state=active`.
      expect(url.searchParams.get('state')).toBe('active');
      expect(url.searchParams.has('cleared')).toBe(false);
      expect(url.searchParams.get('priority_min')).toBe('2');
      expect(url.searchParams.get('priority_max')).toBe('8');

      expect(screen.getByRole('textbox', { name: 'Find an incident' })).toHaveValue('signal');
      openFilters();
      expect(screen.getByRole('radio', { name: 'Planned work' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Active' })).toBeChecked();
    });

    it('restores Ended from initialState, which wins over a legacy initialCleared', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialState="ended" initialCleared="true" />,
      );
      await searchSettled();
      expect(requestUrl(0).searchParams.get('state')).toBe('ended');
    });

    it('ignores an unknown initialState and unparseable priorities rather than sending them', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm
          lines={TEST_LINES}
          tocs={TEST_TOCS}
          initialState="gone"
          initialPriorityMin="not-a-number"
          initialPriorityMax="Infinity"
        />,
      );
      await searchSettled();
      const url = requestUrl(0);
      expect(url.searchParams.has('state')).toBe(false);
      expect(url.searchParams.has('priority_min')).toBe(false);
      expect(url.searchParams.has('priority_max')).toBe(false);
    });

    it.each([
      ['all', null],
      ['7d', 7],
      ['90d', 90],
    ] as const)('restores period=%s', async (period, days) => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialPeriod={period} />);
      await searchSettled();
      const from = requestUrl(0).searchParams.get('from');
      if (days === null) {
        expect(from).toBeNull();
      } else {
        const daysAgo = Math.round((Date.now() - new Date(from as string).getTime()) / (1000 * 60 * 60 * 24));
        expect(Math.abs(daysAgo - days)).toBeLessThanOrEqual(1);
      }
      expect(requestUrl(0).searchParams.has('to')).toBe(false);
    });

    it('restores custom dates with no lower bound from period=custom', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialPeriod="custom" initialTo="2026-09-15" />,
      );
      await searchSettled();
      expect(requestUrl(0).searchParams.has('from')).toBe(false);
      expect(requestUrl(0).searchParams.get('to')).toBe('2026-09-15T22:59:59.999Z');
    });
  });
});
