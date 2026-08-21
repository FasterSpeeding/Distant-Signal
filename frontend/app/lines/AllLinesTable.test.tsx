import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AllLinesTable } from './AllLinesTable';
import type { LineStatusReport, LineSummary } from '@/lib/types';

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

function renderTable() {
  return renderWithMantine(<AllLinesTable lines={lines} reports={reports} pinnedLineIds={[]} />);
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

  it('narrows rows to lines matching the selected operator', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    input.focus();
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'SW' });
    fireEvent.click(option);

    expect(screen.getByText('South Western')).toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
    expect(screen.queryByText('Great Western Railway')).not.toBeInTheDocument();
  });

  it('clearing the filter shows all lines again', async () => {
    renderTable();
    const input = screen.getByRole('combobox', { name: 'Filter by operator' });
    const { fireEvent } = await import('@testing-library/react');
    fireEvent.click(input);
    const option = await screen.findByRole('option', { name: 'SW' });
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
