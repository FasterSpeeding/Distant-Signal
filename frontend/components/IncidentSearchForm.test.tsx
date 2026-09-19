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

const fetchMock = vi.fn();

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockReset();
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
  it('excludes a custom line from the Line dropdown, offering only catalogue lines', () => {
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    const input = screen.getByRole('combobox', { name: /Line \(optional\)/ });
    fireEvent.click(input);
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['South Western Main Line']);
  });

  it('applies a 30-day default "from" floor when no initial filters are given', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

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

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
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
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('Signal failure at Woking')).toBeTruthy();
  });

  it('sends an end-of-day UTC "to" bound so the selected day is genuinely included', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);

    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '2026-09-15' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
    expect(requestedUrl.searchParams.get('to')).toBe('2026-09-15T23:59:59.999Z');
  });

  it('keeps the original filters on a "Load more" request, ignoring a filter change made afterward', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    const [priorityMinInput] = screen.getAllByLabelText('Priority (raw feed value — meaning undocumented)');
    fireEvent.change(priorityMinInput, { target: { value: '2' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Signal failure at Woking');

    // Change a filter AFTER searching but BEFORE "Load more" -- page 2 must
    // still be paginating the original (priority_min=2) search, not this
    // live change.
    fireEvent.change(priorityMinInput, { target: { value: '4' } });

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    const secondRequestUrl = new URL(fetchMock.mock.calls[1][0], 'http://localhost');
    expect(secondRequestUrl.searchParams.get('priority_min')).toBe('2');
    expect(secondRequestUrl.searchParams.get('after')).toBe('cursor-a');
  });

  it('"Load more" appends rows rather than replacing them, and disappears once nextCursor is null', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText("You've reached the end — no more incidents match these filters."),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('does not claim the end of results when a "Load more" page fails -- it reports the failure and keeps the retry', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(errorResponse());

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
    expect(new URL(fetchMock.mock.calls[2][0], 'http://localhost').searchParams.get('after')).toBe('cursor-a');
    expect(screen.queryByText("Couldn't load more results. Try again.")).not.toBeInTheDocument();
    expect(
      screen.getByText("You've reached the end — no more incidents match these filters."),
    ).toBeInTheDocument();
  });

  it('reports a "Load more" whose fetch throws the same way it reports a non-2xx', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      // Lazily, via mockImplementationOnce: a `Promise.reject` built eagerly
      // at mock-setup time is unhandled until the second call consumes it.
      .mockImplementationOnce(() => Promise.reject(new Error('network down')));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Signal failure at Woking');

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(await screen.findByText("Couldn't load more results. Try again.")).toBeInTheDocument();
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
  });

  it('clears a previous "Load more" failure when a fresh search is run', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(errorResponse())
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '3' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Signal failure at Woking');
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await screen.findByText("Couldn't load more results. Try again.");

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

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
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(pageTwo)
      .mockReturnValueOnce(
        okResponse({
          results: [summary({ incidentId: '9', summary: 'Points failure at Woking' })],
          nextCursor: null,
        }),
      );

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('No incidents match these filters.');
    // "Nothing matched" already says everything; it must not be doubled up
    // with the end-of-pagination line.
    expect(screen.queryByText(/You've reached the end/)).not.toBeInTheDocument();
  });

  it('renders an error message on a failed search, not a thrown error', async () => {
    fetchMock.mockReturnValue(errorResponse());
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Search failed');
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
  it('pads the operator and line filter clear buttons to the 24px touch-target floor', () => {
    renderWithMantine(
      <IncidentSearchForm
        lines={TEST_LINES}
        tocs={TEST_TOCS}
        initialOperator="SW"
        initialLine="south-western"
      />,
    );

    expect(screen.getByLabelText('Clear operator filter').className).toContain('iconHitArea24');
    expect(screen.getByLabelText('Clear line filter').className).toContain('iconHitArea24');
  });
});
