import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, within, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AllLinesTable, expandOperatorForFiltering } from './AllLinesTable';
import type { LineStatusReport, LineSummary, Suggestion } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
}));

function report(overrides: Partial<LineStatusReport> & { id: string; name: string }): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    modeName: 'national-rail',
    operators: [],
    computedAt: '2026-07-15T09:00:00Z',
    lineStatuses: [],
    ...overrides,
  };
}

const lines: LineSummary[] = [
  { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
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
        sampleAvailability: { state: 'no-coverage' },
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
        sampleAvailability: { state: 'no-coverage' },
        validityPeriods: [],
        sampleStats: { total: 10, delayed: 5, cancelled: 3, skipped: 0, avgDelayMinutes: 20 },
      },
    ],
  }),
  // swr has no report -> worst/stats/cancelledPct all undefined/null.
];

const tocs: Suggestion[] = [
  { code: 'VT', name: 'Avanti West Coast' },
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
      'GW - Great Western Railway',
      'SW - South Western Railway',
      'VT - Avanti West Coast',
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
    expect(optionText).toEqual(['VT - Avanti West Coast']);
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

describe('AllLinesTable dash tooltip', () => {
  const dashLines: LineSummary[] = [
    { id: 'no-coverage-line', name: 'No Coverage Line', category: 'Regional', operators: ['SW'], source: 'catalogue' },
    { id: 'below-threshold-line', name: 'Below Threshold Line', category: 'Regional', operators: ['SW'], source: 'catalogue' },
    { id: 'tube-line', name: 'Tube Line', category: 'operator', operators: ['TfL'], source: 'catalogue' },
  ];

  function dashReport(id: string, name: string, status: Partial<LineStatusReport['lineStatuses'][number]>): LineStatusReport {
    return report({
      id,
      name,
      lineStatuses: [
        {
          statusSeverity: 10,
          statusSeverityDescription: 'Good Service',
          reason: '',
          dataQuality: 'knowledgebase',
          sampleAvailability: { state: 'no-coverage' },
          validityPeriods: [],
          ...status,
        },
      ],
    });
  }

  const dashReports: LineStatusReport[] = [
    dashReport('no-coverage-line', 'No Coverage Line', { sampleAvailability: { state: 'no-coverage' } }),
    dashReport('below-threshold-line', 'Below Threshold Line', {
      sampleAvailability: { state: 'below-threshold', observed: 2, required: 3 },
    }),
    dashReport('tube-line', 'Tube Line', { dataQuality: 'tfl', sampleAvailability: { state: 'below-threshold', observed: 0, required: 1 } }),
  ];

  function renderDashTable() {
    return renderWithMantine(
      <AllLinesTable lines={dashLines} reports={dashReports} pinnedLineIds={[]} tocs={[]} />,
    );
  }

  it('renders a plain dash glyph for every state, no new visual vocabulary', () => {
    renderDashTable();
    const dashes = screen.getAllByText('—');
    // Two numeric columns (Avg Delay, Cancelled) per row, times three rows.
    expect(dashes.length).toBe(6);
  });

  it('shows the no-coverage reason on hover for a line with zero StationSample rows', async () => {
    renderDashTable();
    const row = screen.getByText('No Coverage Line').closest('tr')!;
    const [avgDelayDash] = within(row).getAllByText('—');
    fireEvent.mouseEnter(avgDelayDash);
    expect(await screen.findByText('No live departure data received for this line yet.')).toBeInTheDocument();
  });

  it('shows the below-threshold reason on hover for a line with thin coverage', async () => {
    renderDashTable();
    const row = screen.getByText('Below Threshold Line').closest('tr')!;
    const [avgDelayDash] = within(row).getAllByText('—');
    fireEvent.mouseEnter(avgDelayDash);
    expect(await screen.findByText('Too few live departures sampled to report a rate right now.')).toBeInTheDocument();
  });

  it('shows the TfL copy on hover, not the below-threshold copy, even though sampleAvailability says below-threshold', async () => {
    renderDashTable();
    const row = screen.getByText('Tube Line').closest('tr')!;
    const [avgDelayDash] = within(row).getAllByText('—');
    fireEvent.mouseEnter(avgDelayDash);
    expect(await screen.findByText("Not measured by this app — status is TfL's own.")).toBeInTheDocument();
  });
});

describe('AllLinesTable responsive columns', () => {
  const mobileLines: LineSummary[] = [
    { id: 'northern', name: 'Northern', category: 'operator', operators: ['NT'], source: 'catalogue' },
  ];

  const mobileReports: LineStatusReport[] = [
    {
      $type: 'DistantSignal.LineStatusReport',
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
          sampleAvailability: { state: 'no-coverage' },
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

  it('hides the Pin column below the sm breakpoint too', () => {
    const { container } = renderMobileTable();
    const hidden = Array.from(container.querySelectorAll('.mantine-visible-from-sm'));
    const text = hidden.map((el) => el.textContent);
    expect(text.some((t) => t?.includes('Pin'))).toBe(true);
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
        $type: 'DistantSignal.LineStatusReport',
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
            sampleAvailability: { state: 'no-coverage' },
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

describe('expandOperatorForFiltering', () => {
  it('expands "TfL" to also include London Overground (LO) and the Elizabeth line (XR)', () => {
    expect(expandOperatorForFiltering('TfL')).toEqual(['TfL', 'LO', 'XR']);
  });

  it('leaves non-"TfL" codes, including LO and XR themselves, unexpanded', () => {
    expect(expandOperatorForFiltering('LO')).toEqual(['LO']);
    expect(expandOperatorForFiltering('XR')).toEqual(['XR']);
    expect(expandOperatorForFiltering('GW')).toEqual(['GW']);
  });
});

describe('AllLinesTable TfL operator filter', () => {
  // Real NR-side operator codes, per crates/common/src/lib.rs's
  // TFL_OPERATOR/TFL_TO_NR_LINE_ID: Tube/DLR/Tram lines are tagged "TfL"
  // directly, while London Overground and the Elizabeth line keep their
  // own NR-side codes ("LO"/"XR") in `operators` even though they're TfL
  // services to a passenger.
  const tflLines: LineSummary[] = [
    { id: 'victoria', name: 'Victoria', category: 'TfL', operators: ['TfL'], source: 'catalogue' },
    { id: 'overground-mildmay', name: 'Mildmay line', category: 'TfL', operators: ['LO'], source: 'catalogue' },
    { id: 'elizabeth-line', name: 'Elizabeth line', category: 'TfL', operators: ['XR'], source: 'catalogue' },
    { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
  ];

  function renderTflTable() {
    return renderWithMantine(<AllLinesTable lines={tflLines} reports={[]} pinnedLineIds={[]} tocs={[]} />);
  }

  it('selecting "TfL" also shows London Overground and Elizabeth line rows', async () => {
    renderTflTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'TfL' });
    fireEvent.click(option);

    expect(screen.getByText('Victoria')).toBeInTheDocument();
    expect(screen.getByText('Mildmay line')).toBeInTheDocument();
    expect(screen.getByText('Elizabeth line')).toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
  });

  it('selecting "LO" alone does not also pull in Tube-only ("TfL") or Elizabeth line ("XR") rows', async () => {
    renderTflTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'LO' });
    fireEvent.click(option);

    expect(screen.getByText('Mildmay line')).toBeInTheDocument();
    expect(screen.queryByText('Victoria')).not.toBeInTheDocument();
    expect(screen.queryByText('Elizabeth line')).not.toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
  });

  it('selecting "XR" alone does not also pull in Tube-only ("TfL") or Overground ("LO") rows', async () => {
    renderTflTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'XR' });
    fireEvent.click(option);

    expect(screen.getByText('Elizabeth line')).toBeInTheDocument();
    expect(screen.queryByText('Victoria')).not.toBeInTheDocument();
    expect(screen.queryByText('Mildmay line')).not.toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
  });

  it('keeps "LO" and "XR" as their own independently-selectable options, not merged into a single "TfL" option', async () => {
    renderTflTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    fireEvent.click(input);
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['LO', 'TfL', 'VT', 'XR']);
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
