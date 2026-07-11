import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { IssueList } from './IssueList';
import type { LineStatus } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

const now = new Date().toISOString();
const future = new Date(Date.now() + 86400000).toISOString();

const minorNow: LineStatus = {
  statusSeverity: 9,
  statusSeverityDescription: 'Minor Delays',
  reason: 'Signal failure',
  dataQuality: 'knowledgebase',
  validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
};

const severePlanned: LineStatus = {
  statusSeverity: 4,
  statusSeverityDescription: 'Planned Closure',
  reason: 'Engineering works',
  dataQuality: 'planned',
  validityPeriods: [{ fromDate: future, toDate: null, isNow: false }],
};

const inferredNow: LineStatus = {
  statusSeverity: 6,
  statusSeverityDescription: 'Severe Delays',
  reason: '10 of 12 sampled services delayed.',
  dataQuality: 'ldbws-inferred',
  validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
};

const all = [minorNow, severePlanned, inferredNow];

describe('IssueList', () => {
  it('renders one row per status, collapsed by default', () => {
    renderWithProvider(<IssueList statuses={all} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
  });

  it('shows a "Now" validity summary on the collapsed row for active statuses', () => {
    renderWithProvider(<IssueList statuses={all} />);
    expect(screen.getAllByText('Now')).toHaveLength(2);
  });

  it('shows the full validity period in the expanded panel', async () => {
    renderWithProvider(<IssueList statuses={[minorNow]} />);
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByText(/Valid:/)).toBeInTheDocument();
  });

  it('filters by severity', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters by source type', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters to active only', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Active'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('filters to upcoming only', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Upcoming'));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
  });

  it('shows a message when no issues match the filters', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    expect(screen.getByText('No issues match the current filters.')).toBeInTheDocument();
  });

  it('expands an entry to reveal its detail on click', async () => {
    const withDisruption: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
      },
    };
    renderWithProvider(<IssueList statuses={[withDisruption]} />);
    expect(screen.queryByText('Full details here')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByText('Full details here')).toBeInTheDocument();
  });
});
