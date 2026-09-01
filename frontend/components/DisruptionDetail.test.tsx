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
  impactType: null,
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

  // `disruption.source` ultimately comes from external feed data, so a
  // path-like incident id (e.g. containing `/`) must be percent-encoded in
  // the link href rather than interpolated raw -- otherwise it could
  // resolve to an unrelated route.
  it('percent-encodes a path-like incident id in the link href', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, source: 'knowledgebase-incident-123/456' }} />,
    );
    const link = screen.getByRole('link', { name: 'View full incident details' });
    expect(link).toHaveAttribute('href', '/incidents/123%2F456');
  });

  it('renders the badge with the correct label for a rail-replacement-bus impact type', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, impactType: 'rail_replacement_bus' }} />,
    );
    expect(screen.getByText('Rail Replacement Bus')).toBeInTheDocument();
  });

  it('renders the badge with the correct label for a no-scheduled-service impact type', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, impactType: 'no_scheduled_service' }} />,
    );
    expect(screen.getByText('No Scheduled Service')).toBeInTheDocument();
  });

  it('renders the badge with the correct label for a diversion impact type', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, impactType: 'diversion' }} />);
    expect(screen.getByText('Diversion')).toBeInTheDocument();
  });

  it('renders no badge when impactType is null', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, impactType: null }} />);
    expect(screen.queryByText('Rail Replacement Bus')).not.toBeInTheDocument();
    expect(screen.queryByText('No Scheduled Service')).not.toBeInTheDocument();
    expect(screen.queryByText('Diversion')).not.toBeInTheDocument();
  });

  it('renders no badge for an unrecognized impactType value, not the raw string', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, impactType: 'some_future_taxonomy_value' }} />,
    );
    expect(screen.queryByText('some_future_taxonomy_value')).not.toBeInTheDocument();
  });
});
