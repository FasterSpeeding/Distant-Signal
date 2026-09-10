import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RealTimeTrainsLink, realTimeTrainsUrl } from './RealTimeTrainsLink';

describe('realTimeTrainsUrl', () => {
  it('builds the gb-nr/detailed service URL from a uid and date', () => {
    expect(realTimeTrainsUrl('W12345', '2026-08-31')).toBe(
      'https://www.realtimetrains.co.uk/service/gb-nr:W12345/2026-08-31/detailed',
    );
  });

  it('encodes a uid that contains URL-unsafe characters', () => {
    expect(realTimeTrainsUrl('W1 2345', '2026-08-31')).toBe(
      'https://www.realtimetrains.co.uk/service/gb-nr:W1%202345/2026-08-31/detailed',
    );
  });
});

describe('RealTimeTrainsLink', () => {
  it('renders a link to the correct Real Time Trains URL for a known uid', () => {
    renderWithMantine(<RealTimeTrainsLink trainUid="W12345" serviceDate="2026-08-31" />);
    expect(screen.getByRole('link', { name: /View on Real Time Trains/ })).toHaveAttribute(
      'href',
      'https://www.realtimetrains.co.uk/service/gb-nr:W12345/2026-08-31/detailed',
    );
  });

  it('opens the link in a new tab with rel="noopener noreferrer"', () => {
    renderWithMantine(<RealTimeTrainsLink trainUid="W12345" serviceDate="2026-08-31" />);
    const link = screen.getByRole('link', { name: /View on Real Time Trains/ });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  });

  it('renders nothing when trainUid is null (not yet resolved to a real service)', () => {
    renderWithMantine(<RealTimeTrainsLink trainUid={null} serviceDate="2026-08-31" />);
    expect(screen.queryByRole('link', { name: /Real Time Trains/ })).not.toBeInTheDocument();
  });
});
