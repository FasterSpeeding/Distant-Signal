import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlatformBadge } from './PlatformBadge';

describe('PlatformBadge', () => {
  it('renders nothing when no platform is known', () => {
    renderWithMantine(<PlatformBadge platform={null} plannedPlatform={null} platformChanged={false} />);
    expect(screen.queryByText(/Platform/)).not.toBeInTheDocument();
  });

  it('shows the current platform alone when it has not changed', () => {
    renderWithMantine(<PlatformBadge platform="6" plannedPlatform="6" platformChanged={false} />);
    expect(screen.getByText('Platform 6')).toBeInTheDocument();
  });

  it('shows the current platform alone when there is no planned value to compare against', () => {
    renderWithMantine(<PlatformBadge platform="6" plannedPlatform={null} platformChanged={false} />);
    expect(screen.getByText('Platform 6')).toBeInTheDocument();
  });

  // WCAG 1.4.1: colour is never the only signal a platform changed -- the
  // badge text itself must name both the current AND the originally
  // planned platform, not rely on the badge's colour alone.
  it('names both the current and originally planned platform in TEXT, not colour alone, when changed', () => {
    renderWithMantine(<PlatformBadge platform="9" plannedPlatform="6" platformChanged={true} />);
    const badge = screen.getByText('Platform 9 (changed from 6)');
    expect(badge).toBeInTheDocument();
  });

  it('gives a changed platform a different colour AND the explanatory text together, never colour alone', () => {
    renderWithMantine(<PlatformBadge platform="9" plannedPlatform="6" platformChanged={true} />);
    const badge = screen.getByText('Platform 9 (changed from 6)');
    // The colour cue (orange) rides alongside the text above -- this
    // assertion just confirms the badge is actually rendered with a
    // non-default colour, not that colour is doing the communicating on
    // its own.
    expect(badge.closest('[data-platform-changed="true"]')).not.toBeNull();
  });

  it('does not mark an unchanged platform as changed', () => {
    renderWithMantine(<PlatformBadge platform="6" plannedPlatform="6" platformChanged={false} />);
    const badge = screen.getByText('Platform 6');
    expect(badge.closest('[data-platform-changed="true"]')).toBeNull();
  });

  // Review §2.4/I20: orange is reserved for lateness elsewhere in the app
  // (the filled "+N MIN" delay badge sits right next to this one on a
  // departure row) -- a changed platform used to share that hue, reading
  // at a glance as a second lateness warning rather than "where to stand".
  it('uses a neutral colour for a changed platform, not the delay-badge orange', () => {
    renderWithMantine(<PlatformBadge platform="9" plannedPlatform="6" platformChanged={true} />);
    const badge = screen.getByText('Platform 9 (changed from 6)').closest('.mantine-Badge-root');
    expect(badge).not.toBeNull();
    // Mantine writes the colour into inline CSS custom properties rather
    // than a `data-color` attribute -- asserting on `style` is the one
    // reliable way to see which colour actually reached the DOM.
    expect(badge?.getAttribute('style')).not.toMatch(/orange/i);
  });

  describe('a cancelled platform', () => {
    it('reads "Platform 4 (cancelled)" to assistive technology, not colour or strike-through alone', () => {
      renderWithMantine(
        <PlatformBadge platform="4" plannedPlatform="4" platformChanged={false} platformStatus="cancelled" />,
      );
      const badge = screen.getByText('Platform 4').closest('.mantine-Badge-root');
      expect(badge).toHaveTextContent('Platform 4 (cancelled)');
      expect(badge).toHaveAttribute('data-platform-cancelled', 'true');
    });

    it('is red and struck through', () => {
      renderWithMantine(
        <PlatformBadge platform="4" plannedPlatform="4" platformChanged={false} platformStatus="cancelled" />,
      );
      const text = screen.getByText('Platform 4');
      expect(text.tagName).toBe('S');
      expect(text.closest('.mantine-Badge-root')?.getAttribute('style')).toMatch(/red/i);
    });

    it('still names a platform change', () => {
      renderWithMantine(
        <PlatformBadge platform="9" plannedPlatform="6" platformChanged={true} platformStatus="cancelled" />,
      );
      expect(screen.getByText('Platform 9 (changed from 6)').closest('.mantine-Badge-root')).toHaveTextContent(
        'Platform 9 (changed from 6) (cancelled)',
      );
    });

    it('renders an active or unknown status as a normal platform', () => {
      renderWithMantine(
        <PlatformBadge
          platform="4"
          plannedPlatform="4"
          platformChanged={false}
          platformStatus={'provisional' as unknown as 'active'}
        />,
      );
      const badge = screen.getByText('Platform 4');
      expect(badge.tagName).not.toBe('S');
      expect(badge.closest('[data-platform-cancelled]')).toBeNull();
    });
  });
});
