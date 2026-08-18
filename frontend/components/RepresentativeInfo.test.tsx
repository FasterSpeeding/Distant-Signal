import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { RepresentativeInfo } from './RepresentativeInfo';
import type { LineStatus } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

function baseStatus(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    ...overrides,
  };
}

describe('RepresentativeInfo', () => {
  it('renders nothing when no status has sampleStats', () => {
    renderWithProvider(<RepresentativeInfo statuses={[baseStatus()]} />);
    // Verify that no component content is rendered (MantineProvider adds styles, but no Card/Text)
    expect(screen.queryByText(/sampled services delayed/)).not.toBeInTheDocument();
  });

  it('renders the sample stats summary when present', () => {
    const withStats = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, skipped: 5, avgDelayMinutes: 12.4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withStats]} />);
    expect(screen.getByText(/142 of 160 sampled services delayed/)).toBeInTheDocument();
    expect(screen.getByText(/3 cancelled/)).toBeInTheDocument();
    expect(screen.getByText(/5 skipping stops/)).toBeInTheDocument();
    expect(screen.getByText(/avg 12\.4 min late/)).toBeInTheDocument();
  });

  it('uses the first status carrying sampleStats when multiple statuses exist', () => {
    const withoutStats = baseStatus();
    const withStats = baseStatus({
      reason: 'Different issue',
      sampleStats: { total: 20, delayed: 5, cancelled: 0, skipped: 0, avgDelayMinutes: 4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withoutStats, withStats]} />);
    expect(screen.getByText(/5 of 20 sampled services delayed/)).toBeInTheDocument();
  });

  it('renders an info trigger explaining what these sample stats represent, distinct from the other info triggers', () => {
    const withStats = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, skipped: 5, avgDelayMinutes: 12.4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withStats]} />);
    const trigger = screen.getByLabelText('About these sample statistics');
    expect(trigger).toBeInTheDocument();
    // Real drawn icon, not the literal "ⓘ" glyph that hits an emoji/font fallback.
    expect(trigger.textContent).not.toContain('ⓘ');
    expect(trigger.querySelector('svg')).toBeInTheDocument();
  });

  it('shows the sampling explanation in the tooltip content on hover', async () => {
    const withStats = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, skipped: 5, avgDelayMinutes: 12.4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withStats]} />);
    fireEvent.mouseEnter(screen.getByLabelText('About these sample statistics'));
    expect(await screen.findByText(/representative/)).toBeInTheDocument();
  });
});
