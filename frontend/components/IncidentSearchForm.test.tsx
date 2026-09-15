import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
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
    affectedStations: ['WOK'],
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
  });

  it('renders the empty-results message, not a blank screen', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('No incidents match these filters.');
  });

  it('renders an error message on a failed search, not a thrown error', async () => {
    fetchMock.mockReturnValue(errorResponse());
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Search failed');
  });
});
