import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { BadgeRow, STATE_TONE_COLOR, StateBadge } from './StateBadge';
import { StatusBadge } from './StatusBadge';
import { StatusCard } from './StatusCard';
import { StatusTile } from './StatusTile';

describe('StateBadge', () => {
  it.each(Object.entries(STATE_TONE_COLOR))('draws the %s tone as a light %s badge', (tone, color) => {
    renderWithMantine(<StateBadge tone={tone as keyof typeof STATE_TONE_COLOR}>Label</StateBadge>);
    const badge = screen.getByText('Label').closest('.mantine-Badge-root')!;
    expect(badge).toHaveAttribute('data-state-badge', tone);
    expect(badge).toHaveAttribute('data-variant', 'light');
    expect(badge.getAttribute('style')).toContain(`--mantine-color-${color}-light`);
  });

  it('is an in-page link when given an href', () => {
    renderWithMantine(<StateBadge href="#leg-2">Needs a train picked</StateBadge>);
    expect(screen.getByRole('link', { name: 'Needs a train picked' })).toHaveAttribute('href', '#leg-2');
  });
});

describe('BadgeRow', () => {
  it('renders severity, then state, then AI, whatever order the props are written in', () => {
    const { container } = renderWithMantine(
      <BadgeRow ai={<span>AI summary</span>} state={<span>Ended</span>} severity={<span>Minor delays</span>} />,
    );
    expect(container.querySelector('[data-badge-row]')?.textContent).toBe('Minor delaysEndedAI summary');
  });

  it('renders nothing with every slot empty', () => {
    const { container } = renderWithMantine(<BadgeRow state={undefined} />);
    expect(container.querySelector('[data-badge-row]')).toBeNull();
  });
});

describe('StatusCard', () => {
  it('is one link with an href, and flat without', () => {
    const { unmount } = renderWithMantine(
      <StatusCard href="/lines/x" title="A line" badge={<StatusBadge severity={9} />} reason="Signal failure" />,
    );
    expect(screen.getByRole('link')).toHaveAttribute('href', '/lines/x');
    unmount();
    renderWithMantine(
      <StatusCard title="An operator" badge={<StatusBadge severity={10} />} footer={<span>Pin</span>} />,
    );
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.getByText('Pin').closest('[data-card-footer]')).not.toBeNull();
  });
});

describe('StatusTile', () => {
  it('links a non-zero count with its own name, and leaves a zero flat', () => {
    const { unmount } = renderWithMantine(
      <StatusTile count={2} label="Minor disruption" href="/lines?statusGroup=mild" ariaLabel="2 lines" />,
    );
    expect(screen.getByRole('link', { name: '2 lines' })).toHaveAttribute('href', '/lines?statusGroup=mild');
    expect(screen.getByText('View lines')).toBeInTheDocument();
    unmount();
    renderWithMantine(<StatusTile count={0} label="Planned" />);
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.queryByText('View lines')).toBeNull();
  });
});
