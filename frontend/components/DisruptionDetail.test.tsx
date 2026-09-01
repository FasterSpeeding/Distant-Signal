import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DisruptionDetail } from './DisruptionDetail';
import type { Disruption } from '@/lib/types';

const sample: Disruption = {
  category: 'RealTime',
  description: 'Signal failure at Woking',
  affectedStops: ['WOK', 'WAT'],
  affectedRoutes: [{ from: 'WAT', to: 'WOK' }],
  source: 'knowledgebase-incident-123',
};

describe('DisruptionDetail', () => {
  it('renders the description', () => {
    renderWithMantine(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('Signal failure at Woking')).toBeInTheDocument();
  });

  it('renders each affected stop', () => {
    renderWithMantine(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('WOK')).toBeInTheDocument();
    expect(screen.getByText('WAT')).toBeInTheDocument();
  });

  it('renders the affected route range', () => {
    renderWithMantine(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
  });

  it('renders nothing extra when affectedRoutes is empty', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, affectedRoutes: [] }} />,
    );
    expect(screen.queryByText(/→/)).not.toBeInTheDocument();
  });

  it('renders the source when present', () => {
    renderWithMantine(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('Source: knowledgebase-incident-123')).toBeInTheDocument();
  });

  it('renders nothing extra when source is null', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: null }} />);
    expect(screen.queryByText(/^Source:/)).not.toBeInTheDocument();
  });

  it('renders a link to the incident detail page when source names a real incident', () => {
    renderWithMantine(<DisruptionDetail disruption={sample} />);
    const link = screen.getByRole('link', { name: 'View full incident details' });
    expect(link).toHaveAttribute('href', '/incidents/123');
  });

  it('renders no incident-detail link when source is the LDBWS-inferred literal', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: 'ldbws-sampling' }} />);
    expect(screen.queryByRole('link', { name: 'View full incident details' })).not.toBeInTheDocument();
  });

  it('renders no incident-detail link when source is a TfL line-keyed value', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: 'tfl-line-status-northern' }} />);
    expect(screen.queryByRole('link', { name: 'View full incident details' })).not.toBeInTheDocument();
  });

  it('renders no incident-detail link when source is null', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: null }} />);
    expect(screen.queryByRole('link', { name: 'View full incident details' })).not.toBeInTheDocument();
  });
});
