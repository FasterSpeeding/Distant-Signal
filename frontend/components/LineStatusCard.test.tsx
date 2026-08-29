import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LineStatusCard } from './LineStatusCard';
import type { LineStatusReport } from '@/lib/types';

const report: LineStatusReport = {
  $type: 'DistantSignal.LineStatusReport',
  id: 'wcml',
  name: 'West Coast Main Line',
  modeName: 'national-rail',
  operators: ['VT'],
  computedAt: '2026-07-15T09:00:00Z',
  lineStatuses: [
    {
      statusSeverity: 9,
      statusSeverityDescription: 'Minor Delays',
      reason: 'Signal failure',
      dataQuality: 'knowledgebase',
      validityPeriods: [{ fromDate: '2026-07-07T10:00:00Z', toDate: null, isNow: true }],
    },
  ],
};

describe('LineStatusCard', () => {
  it('renders the line name', () => {
    renderWithMantine(<LineStatusCard report={report} />);
    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
  });

  it('renders the worst status badge', () => {
    renderWithMantine(<LineStatusCard report={report} />);
    expect(screen.getByText('Minor Delays')).toBeInTheDocument();
  });

  it('renders the status reason', () => {
    renderWithMantine(<LineStatusCard report={report} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
  });

  it('renders a Good Service badge when there are no statuses', () => {
    renderWithMantine(<LineStatusCard report={{ ...report, lineStatuses: [] }} />);
    expect(screen.getByText('Good Service')).toBeInTheDocument();
  });

  it('picks the more severe status when statusSeverity is not monotonic with severity', () => {
    // statusSeverity 10 (GoodService) is numerically lower than 21
    // (Diverted), but Diverted is the actually-worse status — a naive
    // "lowest number wins" comparison would wrongly surface Good Service.
    const mixed: LineStatusReport = {
      ...report,
      lineStatuses: [
        {
          statusSeverity: 10,
          statusSeverityDescription: 'Good Service',
          reason: '',
          dataQuality: 'knowledgebase',
          validityPeriods: [],
        },
        {
          statusSeverity: 21,
          statusSeverityDescription: 'Diverted',
          reason: 'Line diverted due to engineering works',
          dataQuality: 'knowledgebase',
          validityPeriods: [{ fromDate: '2026-07-07T10:00:00Z', toDate: null, isNow: true }],
        },
      ],
    };
    renderWithMantine(<LineStatusCard report={mixed} />);
    expect(screen.getByText('Diverted')).toBeInTheDocument();
    expect(screen.getByText('Line diverted due to engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Good Service')).not.toBeInTheDocument();
  });

  it('renders a last-updated indicator', () => {
    renderWithMantine(<LineStatusCard report={report} />);
    expect(screen.getByText(/Updated (just now|\d+[mhd] ago)/)).toBeInTheDocument();
  });

  it('renders average delay and cancelled percentage when sample stats are present', () => {
    const withStats: LineStatusReport = {
      ...report,
      lineStatuses: [
        {
          ...report.lineStatuses[0],
          sampleStats: { total: 20, delayed: 8, cancelled: 2, skipped: 0, avgDelayMinutes: 7.25 },
        },
      ],
    };
    renderWithMantine(<LineStatusCard report={withStats} />);
    expect(screen.getByText(/Avg delay 7\.3 min/)).toBeInTheDocument();
    expect(screen.getByText(/10% cancelled/)).toBeInTheDocument();
  });

  it('omits the sample stats line entirely when no status carries sample stats', () => {
    renderWithMantine(<LineStatusCard report={report} />);
    expect(screen.queryByText(/Avg delay/)).not.toBeInTheDocument();
  });

  it('clamps a long reason rather than letting it fill the card', () => {
    const wall = 'Station improvement work: '.repeat(40);
    const { container } = renderWithMantine(
      <LineStatusCard report={{ ...report, lineStatuses: [{ ...report.lineStatuses[0], reason: wall }] }} />,
    );
    const reason = container.querySelector('[data-card-reason]') as HTMLElement;
    expect(reason.style.getPropertyValue('-webkit-line-clamp')).toBe('3');
  });

  it('keeps the status badge on the title row even when the name wraps', () => {
    const { container } = renderWithMantine(
      <LineStatusCard report={{ ...report, name: 'Elizabeth line Heathrow Branch' }} />,
    );
    const titleRow = container.querySelector('[data-card-title-row]') as HTMLElement;
    expect(titleRow.getAttribute('data-wrap')).toBe('nowrap');
  });
});
