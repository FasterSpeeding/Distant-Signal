import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RepresentativeInfo } from './RepresentativeInfo';
import type { LineStatus } from '@/lib/types';

function baseStatus(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...overrides,
  };
}

describe('RepresentativeInfo', () => {
  it('renders nothing when no status has sampleStats', () => {
    renderWithMantine(<RepresentativeInfo statuses={[baseStatus()]} />);
    // Verify that no component content is rendered (MantineProvider adds styles, but no Card/Text)
    expect(screen.queryByText(/sampled services delayed/)).not.toBeInTheDocument();
  });

  it('renders the sample stats summary when present', () => {
    const withStats = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, skipped: 5, avgDelayMinutes: 12.4 },
    });
    renderWithMantine(<RepresentativeInfo statuses={[withStats]} />);
    expect(screen.getByText(/142 of 160 sampled services delayed/)).toBeInTheDocument();
    expect(screen.getByText(/3 cancelled \(2%\)/)).toBeInTheDocument();
    expect(screen.getByText(/5 skipping stops/)).toBeInTheDocument();
    expect(screen.getByText(/avg 12\.4 min late/)).toBeInTheDocument();
  });

  it('shows 0% cancelled without dividing by zero when the sample is empty', () => {
    const withStats = baseStatus({
      sampleStats: { total: 0, delayed: 0, cancelled: 0, skipped: 0, avgDelayMinutes: 0 },
    });
    renderWithMantine(<RepresentativeInfo statuses={[withStats]} />);
    expect(screen.getByText(/0 cancelled \(0%\)/)).toBeInTheDocument();
  });

  it('uses the first status carrying sampleStats when multiple statuses exist', () => {
    const withoutStats = baseStatus();
    const withStats = baseStatus({
      reason: 'Different issue',
      sampleStats: { total: 20, delayed: 5, cancelled: 0, skipped: 0, avgDelayMinutes: 4 },
    });
    renderWithMantine(<RepresentativeInfo statuses={[withoutStats, withStats]} />);
    expect(screen.getByText(/5 of 20 sampled services delayed/)).toBeInTheDocument();
  });

  it('prefers fullCoverageStats over sampleStats when both are present on the same status (Decision 1)', () => {
    const both = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, skipped: 5, avgDelayMinutes: 12.4 },
      fullCoverageStats: { total: 500, delayed: 10, cancelled: 1, skipped: 0, avgDelayMinutes: 2.0 },
    });
    renderWithMantine(<RepresentativeInfo statuses={[both]} />);
    expect(screen.getByText(/10 of 500 sampled services delayed/)).toBeInTheDocument();
    expect(screen.queryByText(/142 of 160 sampled services delayed/)).not.toBeInTheDocument();
  });

  it('prefers a status carrying only fullCoverageStats over a sibling carrying only sampleStats', () => {
    const sampleOnly = baseStatus({
      sampleStats: { total: 20, delayed: 5, cancelled: 0, skipped: 0, avgDelayMinutes: 4 },
    });
    const coverageOnly = baseStatus({
      reason: 'Different issue',
      fullCoverageStats: { total: 500, delayed: 10, cancelled: 1, skipped: 0, avgDelayMinutes: 2.0 },
    });
    renderWithMantine(<RepresentativeInfo statuses={[sampleOnly, coverageOnly]} />);
    expect(screen.getByText(/10 of 500 sampled services delayed/)).toBeInTheDocument();
  });

  it('renders the confident provenance note only once fullCoverageStats exists', () => {
    const sampleOnly = baseStatus({
      sampleStats: { total: 20, delayed: 5, cancelled: 0, skipped: 0, avgDelayMinutes: 4 },
    });
    const { unmount } = renderWithMantine(<RepresentativeInfo statuses={[sampleOnly]} />);
    expect(
      screen.queryByText(/Based on real train-movement data for every scheduled service/),
    ).not.toBeInTheDocument();
    unmount();

    const withCoverage = baseStatus({
      fullCoverageStats: { total: 500, delayed: 10, cancelled: 1, skipped: 0, avgDelayMinutes: 2.0 },
    });
    renderWithMantine(<RepresentativeInfo statuses={[withCoverage]} />);
    expect(
      screen.getByText(/Based on real train-movement data for every scheduled service/),
    ).toBeInTheDocument();
  });
});
