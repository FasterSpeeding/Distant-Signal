import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { PlatformBadge } from './PlatformBadge';

describe('PlatformBadge', () => {
  // DS-127: a booked platform with nothing live is provisional, and says so
  // in words (hidden text and tooltip), not by its dotted underline alone.
  describe('a booked, not yet confirmed platform', () => {
    it('shows the booked platform with a "booked, not confirmed" label', () => {
      renderWithMantine(
        <PlatformBadge
          platform={null}
          plannedPlatform={null}
          platformChanged={false}
          bookedPlatform="4"
          platformBasis="booked"
        />,
      );
      const badge = screen.getByText('Platform 4').closest('[data-platform-badge]');
      expect(badge).toHaveAttribute('data-platform-basis', 'booked');
      expect(badge).toHaveAttribute('title', 'Booked platform, not yet shown on station boards');
      expect(badge).toHaveTextContent('Platform 4 (booked, not confirmed)');
      expect(screen.getByText('Platform 4')).toHaveClass('platformBadge__booked');
    });

    it('renders nothing for a booked platform without the booked basis', () => {
      renderWithMantine(
        <PlatformBadge platform={null} plannedPlatform={null} platformChanged={false} bookedPlatform="4" />,
      );
      expect(screen.queryByText(/Platform/)).not.toBeInTheDocument();
    });

    it('prefers a live platform, marked as from a board', () => {
      renderWithMantine(
        <PlatformBadge
          platform="5"
          plannedPlatform="4"
          platformChanged={true}
          bookedPlatform="4"
          platformBasis="board"
        />,
      );
      const badge = screen.getByText('Platform 5 (changed from 4)').closest('[data-platform-badge]');
      expect(badge).toHaveAttribute('data-platform-basis', 'board');
      expect(badge).toHaveAttribute('data-platform-changed', 'true');
    });
  });

  // DS-152: a platform the operator confirmed (Darwin Push Port) says so in
  // words, beside a check glyph that screen readers skip.
  describe('a confirmed platform', () => {
    it('shows a check and a "confirmed" label', () => {
      renderWithMantine(
        <PlatformBadge
          platform="4"
          plannedPlatform="4"
          platformChanged={false}
          bookedPlatform="4"
          platformBasis="confirmed"
        />,
      );
      const badge = screen.getByText('Platform 4').closest('[data-platform-badge]');
      expect(badge).toHaveAttribute('data-platform-basis', 'confirmed');
      expect(badge).toHaveAttribute('title', 'Platform confirmed by the operator');
      expect(badge).toHaveTextContent('Platform 4, confirmed');
      expect(screen.getByText('✓')).toHaveAttribute('aria-hidden', 'true');
    });

    it('keeps a changed platform blue and in words', () => {
      renderWithMantine(
        <PlatformBadge
          platform="9"
          plannedPlatform="6"
          platformChanged={true}
          bookedPlatform="6"
          platformBasis="confirmed"
        />,
      );
      const badge = screen.getByText('Platform 9 (changed from 6)').closest('[data-platform-badge]');
      expect(badge).toHaveAttribute('data-platform-changed', 'true');
      expect(badge).toHaveAttribute('data-platform-basis', 'confirmed');
    });
  });

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

  describe('compact (a narrow table column)', () => {
    it('adds the short phone form, hidden from screen readers, beside the full label', () => {
      renderWithMantine(<PlatformBadge platform="4" plannedPlatform="4" platformChanged={false} compact />);
      expect(screen.getByText('Platform 4')).toHaveClass('phoneLabel__full');
      const short = screen.getByText('4');
      expect(short).toHaveClass('phoneLabel__compact');
      expect(short).toHaveAttribute('aria-hidden', 'true');
    });

    it('keeps a platform change in words: "9 (was 6)"', () => {
      renderWithMantine(<PlatformBadge platform="9" plannedPlatform="6" platformChanged={true} compact />);
      expect(screen.getByText('9 (was 6)').closest('[data-platform-changed="true"]')).not.toBeNull();
      expect(screen.getByText('Platform 9 (changed from 6)')).toBeInTheDocument();
    });

    it('strikes through both forms of a cancelled platform', () => {
      renderWithMantine(
        <PlatformBadge platform="4" plannedPlatform="4" platformChanged={false} platformStatus="cancelled" compact />,
      );
      expect(screen.getByText('Platform 4').closest('s')).not.toBeNull();
      expect(screen.getByText('4').closest('s')).not.toBeNull();
    });
  });

  it('marks itself as never cut short (`data-platform-badge`, globals.css)', () => {
    renderWithMantine(<PlatformBadge platform="4" plannedPlatform="4" platformChanged={false} />);
    expect(screen.getByText('Platform 4').closest('.mantine-Badge-root')).toHaveAttribute('data-platform-badge');
  });
});
