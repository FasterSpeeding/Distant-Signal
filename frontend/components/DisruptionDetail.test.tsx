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

  it('renders safe HTML tags as actual elements, not escaped text', () => {
    const withHtml = { ...sample, description: '<p>Signal failure</p><br/><strong>at Woking</strong>' };
    renderWithMantine(<DisruptionDetail disruption={withHtml} />);
    expect(screen.getByText('Signal failure').tagName).toBe('P');
    expect(screen.getByText('at Woking').tagName).toBe('STRONG');
  });

  it('strips script tags and event handler attributes', () => {
    const malicious = { ...sample, description: '<p onclick="alert(1)">Safe text</p><script>alert(2)</script>' };
    const { container } = renderWithMantine(<DisruptionDetail disruption={malicious} />);
    expect(container.querySelector('script')).not.toBeInTheDocument();
    expect(screen.getByText('Safe text')).not.toHaveAttribute('onclick');
  });

  it('forces target=_blank and rel=noopener on links', () => {
    const withLink = { ...sample, description: '<a href="https://example.com">More info</a>' };
    renderWithMantine(<DisruptionDetail disruption={withLink} />);
    const link = screen.getByText('More info');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener');
  });
});
