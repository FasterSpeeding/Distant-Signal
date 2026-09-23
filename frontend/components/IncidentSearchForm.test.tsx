import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { IncidentSearchForm } from './IncidentSearchForm';
import type { IncidentSearchResponse, IncidentSummary, LineSummary, Suggestion } from '@/lib/types';

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
      <input
        id={`test-date-${label}`}
        value={value ?? ''}
        onChange={(event) => onChange(event.target.value || null)}
      />
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

const pushMock = vi.fn();
const replaceMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, replace: replaceMock }),
  usePathname: () => '/incidents',
}));

const fetchMock = vi.fn();

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockReset();
  pushMock.mockClear();
  replaceMock.mockClear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function okResponse(body: IncidentSearchResponse) {
  return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) } as Response);
}

function errorResponse() {
  return Promise.resolve({ ok: false, status: 500, json: () => Promise.resolve({}) } as Response);
}

/** Clicks the Search button once it reads "Search" again, rather than
 * "Searching…" -- review §3.3's "run the search once on mount" fix means
 * an auto-search is already in flight the instant this form renders, so
 * the button can still show that in-flight label for the one microtask
 * tick between render and this click. `findByRole` (not `getByRole`)
 * waits it out instead of racing it. */
async function clickSearch() {
  fireEvent.click(await screen.findByRole('button', { name: 'Search' }));
}

/** Waits out the same mount-triggered auto-search, for a test that never
 * otherwise interacts with the network. Without this, a test that makes
 * its assertions synchronously (no `await` at all) finishes -- and React
 * Testing Library tears down -- before that request's `.then()` settles
 * and flips `searching` back to `false`, so the resulting state update
 * lands outside any `act()` scope and prints a spurious "not wrapped in
 * act(...)" warning. */
async function awaitMountSettled() {
  await screen.findByRole('button', { name: 'Search' });
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
    ...overrides,
  };
}

describe('IncidentSearchForm', () => {
  it('excludes a custom line from the Line dropdown, offering only catalogue lines', async () => {
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    // Synchronously, with no `await`/`findBy*` in between -- same rationale
    // as the operator `MultiSelect` tests below: an intervening await lets
    // Mantine's floating-ui positioning collapse the just-opened dropdown
    // to `display: none` under jsdom's synthetic layout before the query
    // below ever runs.
    const input = screen.getByRole('combobox', { name: /Line \(optional\)/ });
    fireEvent.click(input);
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['South Western Main Line']);
    await awaitMountSettled();
  });

  it('applies a 30-day default "from" floor when no initial filters are given', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
    const from = requestedUrl.searchParams.get('from');
    expect(from).not.toBeNull();
    const daysAgo = Math.round((Date.now() - new Date(from as string).getTime()) / (1000 * 60 * 60 * 24));
    expect(daysAgo).toBeGreaterThanOrEqual(29);
    expect(daysAgo).toBeLessThanOrEqual(31);
  });

  it('builds a comma-joined operator query parameter from multiple selected operators', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);

    // Select both options from a single dropdown open, querying
    // synchronously (no `await`/`findBy*` between clicks) -- same rationale
    // as `AllLinesTable.test.tsx`'s own MultiSelect tests: Mantine's
    // floating-ui positioning collapses the dropdown to `display: none`
    // under jsdom's synthetic (non-real) layout shortly after open, so a
    // query issued after an intervening `await` sees nothing.
    const input = screen.getByRole('combobox', { name: /Operator \(optional\)/ });
    fireEvent.click(input);
    fireEvent.click(screen.getByRole('option', { name: /SW/ }));
    fireEvent.click(screen.getByRole('option', { name: /VT/ }));

    await clickSearch();
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    // The LAST call, not the first: mount already fired its own auto-search
    // (review §3.3) with the default (no-operator) filter set before this
    // click ever happened.
    const requestedUrl = new URL(fetchMock.mock.calls[fetchMock.mock.calls.length - 1][0], 'http://localhost');
    expect(requestedUrl.searchParams.get('operator')).toBe('SW,VT');
  });

  // The Line filter's whole point is answering "which railway was this?",
  // and `affectedStations` can never answer it -- RDM's Knowledgebase feed
  // carries no station codes, which is why the filter used to return
  // nothing at all (see the 2026-09-16 TfL archive spec, 1c). A result row
  // shows its `affectedLines` by catalogue NAME, falling back to the raw id
  // for a line the catalogue no longer lists.
  it('labels a result row with its affected lines, by name where the catalogue knows them', async () => {
    fetchMock.mockReturnValue(
      okResponse({
        results: [summary({ incidentId: '1', affectedLines: ['south-western', 'retired-line'] })],
        nextCursor: null,
      }),
    );
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    expect(list.textContent).toContain('South Western Main Line');
    expect(list.textContent).toContain('retired-line');
  });

  it('collapses a long affected-lines list into a "+N more" badge', async () => {
    // An operator-only match on a large TOC attributes an incident to every
    // catalogue line that TOC runs -- 13 for Northern -- which would
    // otherwise bury the summary.
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
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    expect(list.textContent).toContain('line-4');
    expect(list.textContent).toContain('+2 more');
    expect(list.textContent).not.toContain('line-5');
    expect(list.textContent).not.toContain('line-6');
  });

  // A rolling deploy can serve this bundle against an api that predates
  // `affectedLines`. Rendering must degrade to "no line badges", never throw
  // and take the whole results list with it.
  it('renders a result row that carries no affectedLines field at all', async () => {
    const { affectedLines: _dropped, ...withoutLines } = summary({ incidentId: '1' });
    fetchMock.mockReturnValue(
      okResponse({ results: [withoutLines as IncidentSummary], nextCursor: null }),
    );
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();

    expect(await screen.findByText('Signal failure at Woking')).toBeTruthy();
  });

  it('sends an end-of-day UTC "to" bound so the selected day is genuinely included', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);

    // The date fields only render once "Custom…" is selected -- see the
    // Period `SegmentedControl` tests below for the collapse itself.
    fireEvent.click(screen.getByRole('radio', { name: 'Custom…' }));
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '2026-09-15' } });
    await clickSearch();

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    // The LAST call: mount's own auto-search (review §3.3) fired first,
    // against the default period (no "to" bound at all).
    const requestedUrl = new URL(fetchMock.mock.calls[fetchMock.mock.calls.length - 1][0], 'http://localhost');
    expect(requestedUrl.searchParams.get('to')).toBe('2026-09-15T23:59:59.999Z');
  });

  it('keeps the original filters on a "Load more" request, ignoring a filter change made afterward', async () => {
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot,
      // against the default (no priority filter) set.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    const priorityMinInput = screen.getByLabelText('Minimum');
    fireEvent.change(priorityMinInput, { target: { value: '2' } });
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    // Change a filter AFTER searching but BEFORE "Load more" -- page 2 must
    // still be paginating the original (priority_min=2) search, not this
    // live change.
    fireEvent.change(priorityMinInput, { target: { value: '4' } });

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
    const secondRequestUrl = new URL(fetchMock.mock.calls[2][0], 'http://localhost');
    expect(secondRequestUrl.searchParams.get('priority_min')).toBe('2');
    expect(secondRequestUrl.searchParams.get('after')).toBe('cursor-a');
  });

  it('"Load more" appends rows rather than replacing them, and disappears once nextCursor is null', async () => {
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');
    expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(1);

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(2));
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();

    // Both pages' rows land in the ONE in-flow list, so the page's own
    // scrollbar reaches every one of them -- see the structural guard
    // below for why that matters.
    const list = document.querySelector('[data-incident-results]') as HTMLElement;
    for (const row of screen.getAllByText('Signal failure at Woking')) {
      expect(list.contains(row)).toBe(true);
    }
  });

  // Regression guard for the archive being hard-clipped at a fixed height.
  // The list used to sit inside a `<ScrollArea mah={520}>`, whose root is
  // `overflow: hidden` while its viewport is `height: 100%`; against a root
  // whose own `height` stays `auto` that percentage resolves to `auto`, so
  // the viewport never overflowed itself (nothing scrolled) and the root
  // clipped everything past 520px. jsdom does no layout, so this asserts
  // the *structure* that caused it instead: the results list must not be
  // inside a Mantine scroll viewport, and no ancestor up to the form may
  // pin a height.
  //
  // Note this also rejects `ScrollArea.Autosize` -- the component that
  // *would* cap the height correctly. That is deliberate rather than
  // incidental: the choice here is "no nested scroller at all, the page
  // scrolls", and `IncidentSearchForm.tsx`'s own comment records why.
  it('renders the results list in the page flow, with no fixed-height or scroll-container ancestor', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    const list = document.querySelector('[data-incident-results]');
    expect(list).not.toBeNull();
    const form = (list as HTMLElement).closest('form');
    expect(form).not.toBeNull();

    // Walk the list itself plus every ancestor up to (and including) the
    // form -- a clip anywhere on that chain hides rows just as effectively
    // as one on the list. (Above the form is this component's caller, which
    // a unit test can't see; `app/incidents/page.tsx` and `app/layout.tsx`
    // are out of scope here.)
    for (
      let node: HTMLElement | null = list as HTMLElement;
      node !== null;
      node = node === form ? null : (node.parentElement as HTMLElement | null)
    ) {
      // Mantine's own scroll viewport, whatever set it up.
      expect(node.hasAttribute('data-scrollarea-viewport')).toBe(false);
      // Mantine resolves a non-responsive `h`/`mah` style prop straight
      // into an inline `height`/`max-height` (`parse-style-props.mjs`), so
      // reading those back off `style` is enough -- no computed style, no
      // layout, which is just as well under jsdom. Verified against the
      // rendered DOM: a `<ScrollArea mah={520}>` root carries
      // `max-height: calc(32.5rem * var(--mantine-scale))`.
      expect(node.style.maxHeight).toBe('');
      expect(node.style.height).toBe('');
      // Only catches a hand-written inline clip -- Mantine's own
      // `overflow: hidden` arrives via the `.m_d57069b5` class, which the
      // `data-scrollarea-viewport` check above is what actually covers.
      expect(node.style.overflow).not.toBe('hidden');
      expect(node.style.overflowY).not.toBe('hidden');
    }

    // The two properties that keep a row from pushing the page sideways
    // once the clipping ancestor is gone. jsdom can't lay anything out, so
    // this is only a tripwire against silent removal -- the reasoning is in
    // `IncidentSearchForm.tsx`'s own comment on this `Group`.
    const header = screen.getByText('Signal failure at Woking').closest('a')
      ?.parentElement as HTMLElement;
    expect(header.style.overflowWrap).toBe('anywhere');
    expect(screen.getByText(/2026/).style.whiteSpace).toBe('nowrap');
  });

  it('says the end has been reached once the last page is in, rather than just dropping the button', async () => {
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');
    // While more pages remain the end-of-results copy must NOT be claimed.
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(
      await screen.findByText("You've reached the end — no more incidents match these filters."),
    ).toBeInTheDocument();
  });

  it('says the end has been reached when the very first page is also the last one', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();

    expect(
      await screen.findByText("You've reached the end — no more incidents match these filters."),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('does not claim the end of results when a "Load more" page fails -- it reports the failure and keeps the retry', async () => {
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(errorResponse());

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
    // The cursor is still valid, so the retry has to still be offered.
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();

    // ...and retrying really does page on from the same cursor, clearing the error.
    fetchMock.mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    await waitFor(() => expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(2));
    // Call 0 is mount's own auto-search, call 1 the explicit Search click,
    // call 2 the failed "Load more", call 3 this retry.
    expect(new URL(fetchMock.mock.calls[3][0], 'http://localhost').searchParams.get('after')).toBe('cursor-a');
    expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument();
    expect(
      screen.getByText("You've reached the end — no more incidents match these filters."),
    ).toBeInTheDocument();
  });

  it('reports a "Load more" whose fetch throws the same way it reports a non-2xx', async () => {
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      // Lazily, via mockImplementationOnce: a `Promise.reject` built eagerly
      // at mock-setup time is unhandled until the third call consumes it.
      .mockImplementationOnce(() => Promise.reject(new Error('network down')));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
  });

  it('clears a previous "Load more" failure when a fresh search is run', async () => {
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(errorResponse())
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '3' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await screen.findByText("Couldn't load more results. Try again.");

    await clickSearch();

    expect(
      await screen.findByText("You've reached the end — no more incidents match these filters."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument();
  });

  it('discards a "Load more" page that lands after a fresh search has already replaced the results', async () => {
    // Search is not disabled while a page is in flight, so this ordering is
    // reachable: page 2 of the OLD search resolves last. Its rows, its cursor
    // and its failure flag all belong to a result set that is no longer on
    // screen and must not be merged into the new one.
    let resolvePageTwo!: (response: Response) => void;
    const pageTwo = new Promise<Response>((resolve) => {
      resolvePageTwo = resolve;
    });
    fetchMock
      // Mount's own auto-search (review §3.3) consumes this first slot.
      .mockReturnValueOnce(okResponse({ results: [], nextCursor: null }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(pageTwo)
      .mockReturnValueOnce(
        okResponse({
          results: [summary({ incidentId: '9', summary: 'Points failure at Woking' })],
          nextCursor: null,
        }),
      );

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Signal failure at Woking');

    fireEvent.click(screen.getByRole('button', { name: 'Load more' })); // page 2 of search 1
    fireEvent.click(screen.getByRole('button', { name: 'Search' })); // search 2
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
      expect(
        screen.getByText("You've reached the end — no more incidents match these filters."),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText('Trespass incident at Woking')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('renders the empty-results message, not a blank screen', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('No incidents match these filters.');
    // "Nothing matched" already says everything; it must not be doubled up
    // with the end-of-pagination line.
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
  });

  it('renders an error message on a failed search, not a thrown error', async () => {
    fetchMock.mockReturnValue(errorResponse());
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    await clickSearch();
    await screen.findByText('Search failed');
    // The click's own (second) request already shows the same text as
    // mount's own auto-search error, so `findByText` above can resolve
    // before this one's response has actually settled -- wait for it too,
    // so its state update lands before the test (and RTL's unmount)
    // finishes, rather than racing cleanup.
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  });

  // Review §2.10: an `InputClearButton` (what `clearable` renders) is a
  // `CloseButton` at its default `size="sm"` (22px) -- a hair under the
  // WCAG 2.5.8 24px target-size floor. `globals.css`'s `.iconHitArea24`
  // pads the invisible hit area up to 24px without growing the visible ×
  // glyph; this only checks the class lands on the rendered button, since
  // jsdom doesn't compute the `::before` pseudo-element's actual painted
  // size. `initialOperator`/`initialLine` seed a value so `clearable`
  // renders the button at all -- it's absent for an empty field.
  // `getByLabelText`, not `getByRole('button', { name: ... })`: Mantine
  // renders this combined clear/chevron section with `aria-hidden="true"`
  // (an existing Mantine behaviour, unrelated to and unchanged by this
  // fix), and `dom-accessibility-api` computes an aria-hidden element's
  // accessible NAME as empty regardless of `getByRole`'s `hidden: true`
  // option (which only un-excludes the *role* match, confirmed against
  // this exact element: `getAllByRole('button', { hidden: true })` lists it
  // by position but its computed name comes back empty) -- `getByLabelText`
  // reads the `aria-label` attribute directly instead. (The date fields'
  // identical fix isn't exercised here: this suite mocks `@mantine/dates`'
  // `DatePickerInput` wholesale, so it never renders a real `CloseButton`
  // to inspect -- see the file-level mock's own comment.)
  it('pads the operator and line filter clear buttons to the 24px touch-target floor', async () => {
    renderWithMantine(
      <IncidentSearchForm
        lines={TEST_LINES}
        tocs={TEST_TOCS}
        initialOperator="SW"
        initialLine="south-western"
      />,
    );
    await awaitMountSettled();

    expect(screen.getByLabelText('Clear operator filter').className).toContain('iconHitArea24');
    expect(screen.getByLabelText('Clear line filter').className).toContain('iconHitArea24');
  });

  // Review §2.13: one idiom for "pick exactly one". The date-range presets
  // used to be a row of filled/light buttons, visually and semantically
  // distinct from the Type/Status `SegmentedControl`s further down the same
  // form -- and none of the three carried a visible caption or an
  // accessible name at all.
  describe('the Period control (review §2.13)', () => {
    it('defaults to the 30-day preset, with the date pickers hidden', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      expect(screen.getByRole('radiogroup', { name: 'Period' })).toBeInTheDocument();
      expect(screen.getByRole('radio', { name: '30 days' })).toBeChecked();
      expect(screen.queryByLabelText('From (optional)')).not.toBeInTheDocument();
      expect(screen.queryByLabelText('To (optional)')).not.toBeInTheDocument();
    });

    it('reveals the From/To date pickers only once Custom… is selected', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      fireEvent.click(screen.getByRole('radio', { name: 'Custom…' }));
      expect(screen.getByLabelText('From (optional)')).toBeInTheDocument();
      expect(screen.getByLabelText('To (optional)')).toBeInTheDocument();
    });

    it('shows Custom… as selected (not an undefined state) when initial filters supply an explicit from date', async () => {
      renderWithMantine(
        <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialFrom="2026-08-01T00:00:00Z" />,
      );
      await awaitMountSettled();
      expect(screen.getByRole('radio', { name: 'Custom…' })).toBeChecked();
      expect(screen.getByLabelText('From (optional)')).toBeInTheDocument();
    });
  });

  // Also Task 1.13's a11y finding for these same two controls: neither
  // carried a visible caption nor an accessible name (no associated
  // `<label>`/`aria-label`/`Input.Wrapper`) before this fix.
  describe('the Type and Status controls (review §2.13/§2.14)', () => {
    it('labels the Type control and gives it an accessible name', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      expect(screen.getByText('Type')).toBeInTheDocument();
      expect(screen.getByRole('radiogroup', { name: 'Type' })).toBeInTheDocument();
    });

    it('labels the Status control and gives it an accessible name', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      expect(screen.getByText('Status')).toBeInTheDocument();
      expect(screen.getByRole('radiogroup', { name: 'Status' })).toBeInTheDocument();
    });
  });

  // Review §2.14: the "raw feed value -- meaning undocumented" caveat used
  // to repeat three times (once per NumberInput label, plus a standalone
  // footnote). Priority stays raw and honestly labelled -- that's still the
  // right call -- but the caveat is said once now, as the wrapper's own
  // description.
  describe('the Priority range control (review §2.14)', () => {
    it('states the raw-feed caveat exactly once', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      expect(screen.getByText('Priority range')).toBeInTheDocument();
      expect(
        screen.getAllByText(/raw feed value from national rail's own incident data/i),
      ).toHaveLength(1);
    });

    it('exposes accessible Minimum/Maximum fields instead of two identically-labelled inputs', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      expect(screen.getByLabelText('Minimum')).toBeInTheDocument();
      expect(screen.getByLabelText('Maximum')).toBeInTheDocument();
    });

    it('still surfaces the min/max validation error once', async () => {
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();
      fireEvent.change(screen.getByLabelText('Minimum'), { target: { value: '10' } });
      fireEvent.change(screen.getByLabelText('Maximum'), { target: { value: '5' } });
      expect(screen.getByText('Minimum must not exceed maximum')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    });
  });

  // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
  // -- on /incidents (as on /trains), a search used to live only in this
  // form's own `useState`, never written back to the URL, so following a
  // result to `/incidents/[incidentId]` and pressing Back lost the search
  // entirely (both the form fields and the results), because Back
  // re-delivers the ORIGINAL, never-updated `/incidents` URL to a brand-new
  // component instance -- a gap "partially masked" here by the existing
  // default-30-day auto-run effect. These tests cover the fix's three moving
  // parts: writing the search to the URL (`router.replace`), reading the
  // four previously-un-seeded filters back OUT of the URL (the new
  // `initialPlanned`/`initialCleared`/`initialPriorityMin`/
  // `initialPriorityMax` props), and the existing mount-only auto-run effect
  // now picking all four up for free. Mirrors
  // `TrainSearchForm.test.tsx`'s identical describe block.
  describe('URL state persistence on search (train-search-state-persistence-design)', () => {
    it('replaces the URL with the query string it just searched, not pushes a new history entry', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);

      // Mount's own auto-search (below) does NOT itself write to the URL --
      // only an explicit Search press does (§3.1 scopes the `router.replace`
      // call to `handleSubmit`).
      await awaitMountSettled();
      expect(replaceMock).not.toHaveBeenCalled();

      // "All time" drops the 30-day default `from` floor, so the query this
      // search SENDS is just this one filter -- but the URL it's replaced
      // with also carries the `period=all` marker (fix round 1), since a
      // dropped `from` alone is indistinguishable from "no filter was ever
      // set" on Back-navigation (see the dedicated test below).
      fireEvent.click(screen.getByRole('radio', { name: 'All time' }));
      fireEvent.change(screen.getByLabelText('Minimum'), { target: { value: '2' } });
      await clickSearch();

      await waitFor(() =>
        expect(replaceMock).toHaveBeenCalledWith('/incidents?priority_min=2&period=all', {
          scroll: false,
        }),
      );
      expect(replaceMock).toHaveBeenCalledTimes(1);
      // The `period` marker must never reach the actual API request --
      // it's a URL-only disambiguator, not part of the wire query.
      const lastFetchUrl = new URL(
        String(fetchMock.mock.calls[fetchMock.mock.calls.length - 1][0]),
        'http://localhost',
      );
      expect(lastFetchUrl.searchParams.has('period')).toBe(false);
      // `replace`, not `push`: this should keep /incidents a single history
      // entry whose URL stays current, not add a new Back-button stop on
      // every search.
      expect(pushMock).not.toHaveBeenCalled();
    });

    it('restores the planned/cleared branch of Type and Status, and both ends of the priority range, from initial props', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm
          lines={TEST_LINES}
          tocs={TEST_TOCS}
          initialPlanned="true"
          initialCleared="true"
          initialPriorityMin="2"
          initialPriorityMax="8"
        />,
      );
      await awaitMountSettled();

      expect(screen.getByRole('radio', { name: 'Planned work' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Cleared' })).toBeChecked();
      expect((screen.getByLabelText('Minimum') as HTMLInputElement).value).toBe('2');
      expect((screen.getByLabelText('Maximum') as HTMLInputElement).value).toBe('8');
    });

    it('restores the realtime/active branch of Type and Status from initial props', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm
          lines={TEST_LINES}
          tocs={TEST_TOCS}
          initialPlanned="false"
          initialCleared="false"
        />,
      );
      await awaitMountSettled();

      expect(screen.getByRole('radio', { name: 'Real-time' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Active' })).toBeChecked();
    });

    it('falls back to blank instead of crashing on an unparseable initialPriorityMin', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialPriorityMin="not-a-number" />,
      );
      await awaitMountSettled();

      expect((screen.getByLabelText('Minimum') as HTMLInputElement).value).toBe('');
      // A garbage value must not have blocked the mount-time auto-search
      // either -- it is treated as absent, not as an error.
      expect(screen.queryByText('Search failed')).not.toBeInTheDocument();
    });

    // Fix round 1, Minor: `Number('')` is `0`, not `NaN`, and `Number
    // .isNaN` alone lets the literal string `'Infinity'` straight through
    // as a "valid" number -- both would otherwise turn a URL that never
    // meant to set a filter (`?priority_min=`, distinct from the param
    // being absent) or a nonsense one (`?priority_min=Infinity`) into a
    // real, active priority bound.
    it('falls back to blank on a present-but-empty initialPriorityMin, rather than treating it as zero', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialPriorityMin="" />);
      await awaitMountSettled();

      expect((screen.getByLabelText('Minimum') as HTMLInputElement).value).toBe('');
    });

    it('falls back to blank on an initialPriorityMax of "Infinity", rather than treating it as a real bound', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} initialPriorityMax="Infinity" />,
      );
      await awaitMountSettled();

      expect((screen.getByLabelText('Maximum') as HTMLInputElement).value).toBe('');
    });

    it('auto-runs the search on mount exactly once, with all four newly-restored filters in the query string', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(
        <IncidentSearchForm
          lines={TEST_LINES}
          tocs={TEST_TOCS}
          initialFrom="2026-08-01T00:00:00Z"
          initialPlanned="true"
          initialCleared="false"
          initialPriorityMin="2"
          initialPriorityMax="8"
        />,
      );

      await awaitMountSettled();
      expect(fetchMock).toHaveBeenCalledTimes(1);
      const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
      expect(requestedUrl.searchParams.get('from')).toBe('2026-08-01T00:00:00.000Z');
      expect(requestedUrl.searchParams.get('planned')).toBe('true');
      expect(requestedUrl.searchParams.get('cleared')).toBe('false');
      expect(requestedUrl.searchParams.get('priority_min')).toBe('2');
      expect(requestedUrl.searchParams.get('priority_max')).toBe('8');
      // The mount effect itself must not touch the URL -- only an explicit
      // Search press does (see the `router.replace` test above).
      expect(replaceMock).not.toHaveBeenCalled();
    });

    // Fix round 1, Important: a restored URL with no `from` at all used to
    // be indistinguishable from "no filter was ever set", so Back-
    // navigation after an "All time" search silently fell back to the
    // 30-day default instead -- a materially different, narrower search,
    // with nothing on screen to say so. `initialPeriod="all"` is the new
    // marker that breaks the tie.
    it('restores "All time" (not the 30-day default) from initialPeriod="all", with both date fields cleared', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      // Paired with `initialOperator` so the mount effect's `query` guard
      // (empty query -> no auto-search, see its own comment) doesn't skip
      // the search entirely: a bare "All time" with nothing else set is a
      // genuinely empty filter set, not something this test needs to prove
      // separately.
      renderWithMantine(
        <IncidentSearchForm
          lines={TEST_LINES}
          tocs={TEST_TOCS}
          initialOperator="SW"
          initialPeriod="all"
        />,
      );
      await awaitMountSettled();

      expect(screen.getByRole('radio', { name: 'All time' })).toBeChecked();
      // The date pickers only render once "Custom…" is selected -- "All
      // time" being checked and NOT "Custom…" is itself proof both dates
      // are unset, but this also confirms the mount-time auto-search (which
      // already ran by the time `awaitMountSettled` resolves) went out with
      // no lower/upper bound at all.
      expect(screen.queryByLabelText('From (optional)')).not.toBeInTheDocument();
      expect(screen.queryByLabelText('To (optional)')).not.toBeInTheDocument();
      const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
      expect(requestedUrl.searchParams.get('operator')).toBe('SW');
      expect(requestedUrl.searchParams.has('from')).toBe(false);
      expect(requestedUrl.searchParams.has('to')).toBe(false);
    });

    it('does not let a plain, unfiltered first visit fall through initialPeriod into "All time" -- it still gets the 30-day default', async () => {
      fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
      renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
      await awaitMountSettled();

      expect(screen.getByRole('radio', { name: '30 days' })).toBeChecked();
    });
  });
});
