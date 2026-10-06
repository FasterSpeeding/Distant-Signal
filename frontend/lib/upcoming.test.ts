import { describe, it, expect } from 'vitest';
import { formatUpcomingWhen } from './upcoming';

describe('formatUpcomingWhen', () => {
  it('reads a whole London day as that day', () => {
    // Sunday 11 October 2026, BST: 10 Oct 23:00Z to 11 Oct 23:00Z.
    expect(formatUpcomingWhen({ from: '2026-10-10T23:00:00Z', to: '2026-10-11T23:00:00Z' })).toBe('Sun 11 Oct');
  });

  it('handles the 25-hour day the clocks go back', () => {
    expect(formatUpcomingWhen({ from: '2026-10-24T23:00:00Z', to: '2026-10-26T00:00:00Z' })).toBe('Sun 25 Oct');
  });

  it('reads several whole days as a range', () => {
    expect(formatUpcomingWhen({ from: '2026-10-11T23:00:00Z', to: '2026-10-14T23:00:00Z' })).toBe(
      'Mon 12 Oct to Wed 14 Oct',
    );
  });

  it('gives times for anything else', () => {
    expect(formatUpcomingWhen({ from: '2026-10-16T04:00:00Z', to: '2026-10-16T22:30:00Z' })).toBe(
      'Fri 16 Oct, 05:00 to Fri 16 Oct, 23:30',
    );
    expect(formatUpcomingWhen({ from: '2026-10-16T04:00:00Z', to: null })).toBe('from Fri 16 Oct, 05:00');
  });
});
