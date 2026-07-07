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
});
