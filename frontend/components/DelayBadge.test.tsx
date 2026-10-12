import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DELAY_TONE_COLOR, DelayBadge } from './DelayBadge';

describe('DelayBadge', () => {
  it.each([
    [12, false, '12 min late', 'orange'],
    [12, true, 'Exp. 12 min late', 'orange'],
    [-3, false, '3 min early', 'teal'],
    [0, false, 'On time', 'green'],
  ])('%i min (provisional %s) reads "%s" on a light %s badge', (delayMinutes, provisional, text, color) => {
    renderWithMantine(<DelayBadge delayMinutes={delayMinutes} provisional={provisional} />);
    const badge = screen.getByText(text).closest('.mantine-Badge-root');
    expect(badge).toHaveAttribute('data-variant', 'light');
    expect(badge).toHaveStyle({ '--badge-bg': `var(--mantine-color-${color}-light)` });
  });

  it("is sentence case, never Mantine's default uppercase", () => {
    renderWithMantine(<DelayBadge delayMinutes={5} />);
    expect(screen.getByText('5 min late').closest('.mantine-Badge-root')).toHaveStyle({ textTransform: 'none' });
  });

  it('holds the one delay-to-colour table', () => {
    expect(DELAY_TONE_COLOR).toEqual({ late: 'orange', early: 'teal', onTime: 'green' });
  });
});
