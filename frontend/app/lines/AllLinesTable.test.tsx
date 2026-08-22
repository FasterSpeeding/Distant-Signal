import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AllLinesTable } from './AllLinesTable';
import type { LineStatusReport, LineSummary } from '@/lib/types';

// AllLinesTable's Pin column renders PinToggle, which calls useRouter()
// from next/navigation -- that throws "invariant expected app router to be
// mounted" outside a real Next.js App Router tree. Stub it the same way
// components/PinToggle.test.tsx does; router.refresh() isn't under test here.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
}));

const lines: LineSummary[] = [
  { id: 'northern', name: 'Northern', category: 'operator', operators: ['NT'], source: 'catalogue' },
];

const reports: LineStatusReport[] = [
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

function renderTable() {
  return renderWithMantine(
    <AllLinesTable lines={lines} reports={reports} pinnedLineIds={[]} tocs={[]} />,
  );
}

describe('AllLinesTable responsive columns', () => {
  it('renders the full status label, never an ellipsized stub', () => {
    renderTable();
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
  });

  it('hides the numeric columns below the sm breakpoint', () => {
    const { container } = renderTable();
    const hidden = Array.from(container.querySelectorAll('.mantine-visible-from-sm'));
    const text = hidden.map((el) => el.textContent);
    expect(text.some((t) => t?.includes('Avg Delay'))).toBe(true);
    expect(text.some((t) => t?.includes('Cancelled'))).toBe(true);
  });

  it('re-surfaces the numbers under the line name for the widths that lose the columns', () => {
    const { container } = renderTable();
    const mobileOnly = container.querySelector('.mantine-hidden-from-sm');
    expect(mobileOnly).not.toBeNull();
    expect(mobileOnly!.textContent).toContain('7.5 min');
    expect(mobileOnly!.textContent).toContain('20%');
  });

  it('says so explicitly when a line has no sample data', () => {
    const { container } = renderWithMantine(
      <AllLinesTable lines={lines} reports={[]} pinnedLineIds={[]} tocs={[]} />,
    );
    expect(container.querySelector('.mantine-hidden-from-sm')!.textContent).toContain('No sample data');
  });
});
