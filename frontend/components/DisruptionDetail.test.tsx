import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { DisruptionDetail } from './DisruptionDetail';
import type { Disruption } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

const sample: Disruption = {
  category: 'RealTime',
  description: 'Signal failure at Woking',
  affectedStops: ['WOK', 'WAT'],
  affectedRoutes: [{ from: 'WAT', to: 'WOK' }],
  source: 'knowledgebase-incident-123',
};

describe('DisruptionDetail', () => {
  it('renders the description', () => {
    renderWithProvider(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('Signal failure at Woking')).toBeInTheDocument();
  });

  it('renders each affected stop', () => {
    renderWithProvider(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('WOK')).toBeInTheDocument();
    expect(screen.getByText('WAT')).toBeInTheDocument();
  });

  it('renders the affected route range', () => {
    renderWithProvider(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
  });

  it('renders nothing extra when affectedRoutes is empty', () => {
    renderWithProvider(
      <DisruptionDetail disruption={{ ...sample, affectedRoutes: [] }} />,
    );
    expect(screen.queryByText(/→/)).not.toBeInTheDocument();
  });
});
