import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { LineStatusCard } from './LineStatusCard';
import type { LineStatusReport } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

const report: LineStatusReport = {
  $type: 'NRStatus.LineStatusReport',
  id: 'wcml',
  name: 'West Coast Main Line',
  modeName: 'national-rail',
  operators: ['AW'],
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
    renderWithProvider(<LineStatusCard report={report} />);
    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
  });

  it('renders the worst status badge', () => {
    renderWithProvider(<LineStatusCard report={report} />);
    expect(screen.getByText('Minor Delays')).toBeInTheDocument();
  });

  it('renders the status reason', () => {
    renderWithProvider(<LineStatusCard report={report} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
  });

  it('renders a Good Service badge when there are no statuses', () => {
    renderWithProvider(<LineStatusCard report={{ ...report, lineStatuses: [] }} />);
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
    renderWithProvider(<LineStatusCard report={mixed} />);
    expect(screen.getByText('Diverted')).toBeInTheDocument();
    expect(screen.getByText('Line diverted due to engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Good Service')).not.toBeInTheDocument();
  });

  it('renders a last-updated indicator', () => {
    renderWithProvider(<LineStatusCard report={report} />);
    expect(screen.getByText(/Updated (just now|\d+[mhd] ago)/)).toBeInTheDocument();
  });
});
