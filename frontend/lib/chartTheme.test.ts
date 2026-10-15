import { describe, expect, it } from 'vitest';
import {
  bucketTickLabel,
  bucketTooltipLabel,
  CHART_COLORS,
  CHART_CURVE,
  formatCount,
  formatMinutes,
  formatRate,
} from './chartTheme';

describe('chart number formats', () => {
  it('groups thousands in counts', () => {
    expect(formatCount(80000)).toBe('80,000');
    expect(formatCount(1234567)).toBe('1,234,567');
    expect(formatCount(42)).toBe('42');
  });

  it('shows rates as percentages with at most one decimal', () => {
    expect(formatRate(0.1)).toBe('10%');
    expect(formatRate(0.1234)).toBe('12.3%');
    expect(formatRate(0)).toBe('0%');
  });

  it('shows minutes with the site unit and a thousands separator', () => {
    expect(formatMinutes(2.5)).toBe('2.5 min');
    expect(formatMinutes(3)).toBe('3 min');
    expect(formatMinutes(1200)).toBe('1,200 min');
  });
});

describe('bucket labels', () => {
  it('labels a day tick with a short date and its tooltip with the full date', () => {
    expect(bucketTickLabel('day', '2026-09-15')).toBe('15 Sep');
    expect(bucketTooltipLabel('day', '2026-09-15')).toBe('15 Sep 2026');
  });

  it('labels a sub-day tick with the UK time and its tooltip with the date too', () => {
    expect(bucketTickLabel('halfHour', '2026-09-15T13:30:00Z')).toBe('14:30');
    expect(bucketTooltipLabel('halfHour', '2026-09-15T13:30:00Z')).toBe('15 Sep, 14:30');
  });
});

describe('chart settings', () => {
  it('draws straight lines, never a smoothed curve', () => {
    expect(CHART_CURVE).toBe('linear');
  });

  it('takes every series colour from a theme-aware CSS variable', () => {
    for (const color of Object.values(CHART_COLORS)) {
      expect(color).toMatch(/^var\(--ds-chart-[a-z]+\)$/);
    }
  });
});
