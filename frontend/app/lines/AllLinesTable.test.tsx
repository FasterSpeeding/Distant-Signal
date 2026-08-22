import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, within, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AllLinesTable } from './AllLinesTable';
import type { LineStatusReport, LineSummary, Suggestion } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
}));

function report(overrides: Partial<LineStatusReport> & { id: string; name: string }): LineStatusReport {
  return {
    $type: 'NRStatus.LineStatusReport',
    modeName: 'national-rail',
    operators: [],
    computedAt: '2026-07-15T09:00:00Z',
    lineStatuses: [],
    ...overrides,
  };
}

const lines: LineSummary[] = [
  { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['AW'], source: 'catalogue' },
  { id: 'gwr', name: 'Great Western Railway', category: 'Long Distance', operators: ['GW'], source: 'catalogue' },
  { id: 'swr', name: 'South Western', category: 'Regional', operators: ['SW', 'GW'], source: 'catalogue' },
];

const reports: LineStatusReport[] = [
  report({
    id: 'wcml',
    name: 'West Coast Main Line',
    lineStatuses: [
      {
        statusSeverity: 9,
        statusSeverityDescription: 'Minor Delays',
        reason: '',
        dataQuality: 'knowledgebase',
        validityPeriods: [],
        sampleStats: { total: 10, delayed: 2, cancelled: 1, skipped: 0, avgDelayMinutes: 5 },
      },
    ],
  }),
  report({
    id: 'gwr',
    name: 'Great Western Railway',
    lineStatuses: [
      {
        statusSeverity: 2,
        statusSeverityDescription: 'Suspended',
        reason: '',
        dataQuality: 'knowledgebase',
        validityPeriods: [],
        sampleStats: { total: 10, delayed: 5, cancelled: 3, skipped: 0, avgDelayMinutes: 20 },
      },
    ],
  }),
  // swr has no report -> worst/stats/cancelledPct all undefined/null.
];

const tocs: Suggestion[] = [
  { code: 'AW', name: 'Avanti West Coast' },
  { code: 'GW', name: 'Great Western Railway' },
  { code: 'SW', name: 'South Western Railway' },
];

function renderTable() {
  return renderWithMantine(<AllLinesTable lines={lines} reports={reports} pinnedLineIds={[]} tocs={tocs} />);
}

function rowNames() {
  const rows = screen.getAllByRole('row').slice(1); // skip header row
  return rows.map((row) => within(row).getAllByRole('link')[0].textContent);
}

describe('AllLinesTable', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not render Category or Operators columns', () => {
    renderTable();
    expect(screen.queryByText('Category')).not.toBeInTheDocument();
    expect(screen.queryByText('Operators')).not.toBeInTheDocument();
    expect(screen.queryByText('Long Distance')).not.toBeInTheDocument();
  });

  it('renders all lines when no operator filter is applied', () => {
    renderTable();
    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
    expect(screen.getByText('Great Western Railway')).toBeInTheDocument();
    expect(screen.getByText('South Western')).toBeInTheDocument();
  });

  it('allows typing into the operator filter input', () => {
    // `fireEvent.change` below bypasses the `readOnly` attribute the way a
    // real keystroke would not -- Mantine's MultiSelect renders its input
    // `readOnly` unless `searchable` is passed, which blocks actual typing
    // in a browser even though jsdom's synthetic change events still fire.
    // Assert on the attribute directly so this test fails without `searchable`.
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    expect(input).not.toHaveAttribute('readonly');
  });

  it('shows operator options as "CODE - Full Name"', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    // Query synchronously, right after opening the dropdown, and read every
    // option in one pass -- Mantine's floating-ui positioning collapses the
    // dropdown to `display: none` in jsdom shortly after open (no real
    // layout/IntersectionObserver here), so a second query issued after an
    // earlier `await` sees nothing. One synchronous snapshot avoids that.
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual([
      'AW - Avanti West Coast',
      'GW - Great Western Railway',
      'SW - South Western Railway',
    ]);
  });

  it('narrows rows to lines matching the selected operator', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    input.focus();
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'SW - South Western Railway' });
    fireEvent.click(option);

    expect(screen.getByText('South Western')).toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
    expect(screen.queryByText('Great Western Railway')).not.toBeInTheDocument();
  });

  it('supports searching the operator filter by typing a name instead of a code', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    fireEvent.change(input, { target: { value: 'Avanti' } });

    // Synchronous, single-pass query -- see the comment on the option-label
    // test above for why this can't be split across an `await`.
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['AW - Avanti West Coast']);
  });

  it('supports searching the operator filter by typing a code instead of a name', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    fireEvent.change(input, { target: { value: 'SW' } });

    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['SW - South Western Railway']);
  });

  it('clearing the filter shows all lines again', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'SW - South Western Railway' });
    fireEvent.click(option);
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();

    // Clicking the already-selected option again is MultiSelect's standard
    // toggle-off interaction -- exercises the same "no operators selected"
    // state the dedicated clear button produces.
    fireEvent.click(option);

    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
    expect(screen.getByText('Great Western Railway')).toBeInTheDocument();
    expect(screen.getByText('South Western')).toBeInTheDocument();
  });

  it('sorts ascending by name on first click, descending on second click', async () => {
    renderTable();
    const { fireEvent } = await import('@testing-library/react');
    const header = screen.getByText('Name');

    fireEvent.click(header);
    expect(rowNames()).toEqual(['Great Western Railway', 'South Western', 'West Coast Main Line']);

    fireEvent.click(header);
    expect(rowNames()).toEqual(['West Coast Main Line', 'South Western', 'Great Western Railway']);
  });

  it('resets to ascending when switching to a different column', async () => {
    renderTable();
    const { fireEvent } = await import('@testing-library/react');

    const nameHeader = screen.getByText(/^Name/);
    fireEvent.click(nameHeader);
    fireEvent.click(nameHeader); // now descending by name

    const delayHeader = screen.getByText(/^Avg Delay/);
    fireEvent.click(delayHeader);

    // Ascending by avg delay: wcml (5) < gwr (20) < swr (no stats, sorts last).
    expect(rowNames()).toEqual(['West Coast Main Line', 'Great Western Railway', 'South Western']);
  });

  it('sorts rows with no data to the end regardless of direction', async () => {
    renderTable();
    const { fireEvent } = await import('@testing-library/react');
    const delayHeader = screen.getByText(/^Avg Delay/);

    fireEvent.click(delayHeader); // asc
    expect(rowNames()[2]).toBe('South Western');

    fireEvent.click(delayHeader); // desc
    expect(rowNames()[2]).toBe('South Western');
  });
});

describe('AllLinesTable responsive columns', () => {
  const mobileLines: LineSummary[] = [
    { id: 'northern', name: 'Northern', category: 'operator', operators: ['NT'], source: 'catalogue' },
  ];

  const mobileReports: LineStatusReport[] = [
    {
      $type: 'NRStatus.LineStatusReport',
      id: 'northern',
      name: 'Northern',
      modeName: 'national-rail',
      operators: ['NT'],
      computedAt: '2026-08-21T12:00:00Z',
      lineStatuses: [
        {
          statusSeverity: 6,
          statusSeverityDescription: 'Severe Delays',
          reason: 'Signalling failure',
          dataQuality: 'ldbws-inferred',
          validityPeriods: [{ fromDate: '2026-08-21T09:00:00Z', toDate: null, isNow: true }],
          sampleStats: { total: 10, delayed: 4, cancelled: 2, skipped: 0, avgDelayMinutes: 7.5 },
        },
      ],
    },
  ];

  function renderMobileTable() {
    return renderWithMantine(
      <AllLinesTable lines={mobileLines} reports={mobileReports} pinnedLineIds={[]} tocs={[]} />,
    );
  }

  it('renders the full status label, never an ellipsized stub', () => {
    renderMobileTable();
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
  });

  it('hides the numeric columns below the sm breakpoint', () => {
    const { container } = renderMobileTable();
    const hidden = Array.from(container.querySelectorAll('.mantine-visible-from-sm'));
    const text = hidden.map((el) => el.textContent);
    expect(text.some((t) => t?.includes('Avg Delay'))).toBe(true);
    expect(text.some((t) => t?.includes('Cancelled'))).toBe(true);
  });

  it('re-surfaces the numbers under the line name for the widths that lose the columns', () => {
    const { container } = renderMobileTable();
    const mobileOnly = container.querySelector('.mantine-hidden-from-sm');
    expect(mobileOnly).not.toBeNull();
    expect(mobileOnly!.textContent).toContain('7.5 min');
    expect(mobileOnly!.textContent).toContain('20%');
  });

  it('says so explicitly when a line has no sample data', () => {
    const { container } = renderWithMantine(
      <AllLinesTable lines={mobileLines} reports={[]} pinnedLineIds={[]} tocs={[]} />,
    );
    expect(container.querySelector('.mantine-hidden-from-sm')!.textContent).toContain('No sample data');
  });

  it('never renders the literal string "null" when a line has stats but zero samples', () => {
    // stats.total === 0 is a real, permitted case per SampleStats -- it
    // makes cancelledPct null even though `stats` itself is truthy, which
    // the mobile summary text must guard against separately from `stats`.
    const zeroSampleReports: LineStatusReport[] = [
      {
        $type: 'NRStatus.LineStatusReport',
        id: 'northern',
        name: 'Northern',
        modeName: 'national-rail',
        operators: ['NT'],
        computedAt: '2026-08-21T12:00:00Z',
        lineStatuses: [
          {
            statusSeverity: 10,
            statusSeverityDescription: 'Good Service',
            reason: '',
            dataQuality: 'ldbws-inferred',
            validityPeriods: [],
            sampleStats: { total: 0, delayed: 0, cancelled: 0, skipped: 0, avgDelayMinutes: 0 },
          },
        ],
      },
    ];
    const { container } = renderWithMantine(
      <AllLinesTable lines={mobileLines} reports={zeroSampleReports} pinnedLineIds={[]} tocs={[]} />,
    );
    const mobileOnly = container.querySelector('.mantine-hidden-from-sm');
    expect(mobileOnly).not.toBeNull();
    expect(mobileOnly!.textContent).not.toContain('null');
  });
});

describe('AllLinesTable sorting affordance', () => {
  it('shows a sort glyph on every sortable header before anything is clicked', () => {
    renderTable();
    expect(screen.getAllByText('↕').length).toBeGreaterThanOrEqual(3);
  });

  it('makes the headers real buttons, so they are keyboard-operable', () => {
    renderTable();
    const nameHeader = screen.getByRole('button', { name: /Name/ });
    expect(nameHeader).toBeInTheDocument();
  });

  it('announces sort state via aria-sort', () => {
    renderTable();
    const header = screen.getByRole('columnheader', { name: /Name/ });
    expect(header).toHaveAttribute('aria-sort', 'none');
    fireEvent.click(screen.getByRole('button', { name: /Name/ }));
    expect(header).toHaveAttribute('aria-sort', 'ascending');
    fireEvent.click(screen.getByRole('button', { name: /Name/ }));
    expect(header).toHaveAttribute('aria-sort', 'descending');
  });
});
