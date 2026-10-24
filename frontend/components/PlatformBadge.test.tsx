import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { badgeTextTransform, spokenText } from '@/test/badge';
import type { PlatformFacts } from '@/lib/platformLabel';
import { PlatformBadge } from './PlatformBadge';

const LIVE: PlatformFacts = { platform: '4', plannedPlatform: '4', platformChanged: false };
const CHANGED: PlatformFacts = { platform: '9', plannedPlatform: '6', platformChanged: true };
const BOOKED: PlatformFacts = {
  platform: null,
  plannedPlatform: null,
  platformChanged: false,
  bookedPlatform: '4',
  platformBasis: 'booked',
};

/** Render one badge and return its root. */
function badge(facts: PlatformFacts, compact = false): HTMLElement {
  renderWithMantine(<PlatformBadge {...facts} compact={compact} />);
  const root = document.querySelector<HTMLElement>('[data-platform-badge]');
  if (root === null) throw new Error('no platform badge rendered');
  return root;
}

/** The text drawn on screen from `sm` up: the badge's text without the
 * visually hidden words or the phone form. */
function drawnText(root: HTMLElement): string {
  const copy = root.cloneNode(true) as HTMLElement;
  for (const el of copy.querySelectorAll('.phoneLabel__compact, [class*="VisuallyHidden"]')) el.remove();
  return copy.textContent;
}

/** Mantine writes the colour into the root's inline CSS variables. */
function colourOf(root: HTMLElement): string {
  return root.getAttribute('style') ?? '';
}

describe('PlatformBadge', () => {
  it('renders nothing when no platform is known', () => {
    renderWithMantine(<PlatformBadge platform={null} plannedPlatform={null} platformChanged={false} />);
    expect(document.querySelector('[data-platform-badge]')).toBeNull();
  });

  // DS-127: a booked platform with nothing live is provisional, and says
  // so in words on screen, not by its outline or dotted underline alone.
  describe('booked', () => {
    it('is a gray outline badge reading "Platform 4 (booked)", the number underlined in dots', () => {
      const root = badge(BOOKED);
      expect(root).toHaveAttribute('data-platform-state', 'booked');
      expect(root).toHaveAttribute('data-variant', 'outline');
      expect(colourOf(root)).toMatch(/gray/);
      expect(drawnText(root)).toBe('Platform 4 (booked)');
      expect(screen.getByText('Platform 4')).toHaveClass('platformBadge__booked');
    });

    it('is spoken as booked, not confirmed, with a tooltip saying why', () => {
      const root = badge(BOOKED);
      expect(spokenText(root)).toBe('Platform 4 (booked, not confirmed)');
      expect(root).toHaveAttribute('title', 'Booked platform, not yet shown on station boards');
    });

    it('needs the booked basis', () => {
      renderWithMantine(<PlatformBadge {...BOOKED} platformBasis={null} />);
      expect(document.querySelector('[data-platform-badge]')).toBeNull();
    });
  });

  describe('live (shown on a board, unchanged)', () => {
    it('is a gray light badge reading "Platform 4", with no tooltip', () => {
      const root = badge(LIVE);
      expect(root).toHaveAttribute('data-platform-state', 'live');
      expect(root).toHaveAttribute('data-variant', 'light');
      expect(root).toHaveAttribute('data-platform-changed', 'false');
      expect(colourOf(root)).toMatch(/gray/);
      expect(drawnText(root)).toBe('Platform 4');
      expect(spokenText(root)).toBe('Platform 4');
      expect(root).not.toHaveAttribute('title');
    });

    it('reads as live when there is no earlier platform to compare against', () => {
      const root = badge({ ...LIVE, plannedPlatform: null });
      expect(root).toHaveAttribute('data-platform-state', 'live');
    });
  });

  describe('confirmed', () => {
    it('is a filled gray badge, spoken as confirmed', () => {
      const root = badge({ ...LIVE, platformBasis: 'confirmed' });
      expect(root).toHaveAttribute('data-platform-state', 'live');
      expect(root).toHaveAttribute('data-platform-confirmed', 'true');
      expect(root).toHaveAttribute('data-variant', 'filled');
      expect(colourOf(root)).toMatch(/gray/);
      expect(drawnText(root)).toBe('Platform 4');
      expect(spokenText(root)).toBe('Platform 4 (confirmed)');
      expect(root).toHaveAttribute('title', 'Confirmed platform');
    });

    it('is not marked confirmed for a board platform', () => {
      expect(badge({ ...LIVE, platformBasis: 'board' })).not.toHaveAttribute('data-platform-confirmed');
    });
  });

  describe('changed', () => {
    it('is a blue light badge with the old number struck through before the new one', () => {
      const root = badge(CHANGED);
      expect(root).toHaveAttribute('data-platform-state', 'changed');
      expect(root).toHaveAttribute('data-platform-changed', 'true');
      expect(root).toHaveAttribute('data-variant', 'light');
      expect(drawnText(root)).toBe('Platform 6 9');
      const struck = root.querySelectorAll('s');
      expect(struck).toHaveLength(1);
      expect(struck[0]).toHaveTextContent('6');
      expect(struck[0]).toHaveClass('platformBadge__previous');
    });

    // WCAG 1.4.1: never colour alone, and never a bare struck number for a
    // screen reader, which announces neither.
    it('says the change in words: spoken, and in its tooltip', () => {
      const root = badge(CHANGED);
      expect(spokenText(root)).toBe('Platform 9 (changed from 6)');
      expect(root).toHaveAttribute('title', 'Changed from platform 6');
      expect(root.querySelector('s')?.closest('[aria-hidden="true"]')).not.toBeNull();
    });

    // Review §2.4/I20: orange means lateness, and a delay badge sits right
    // beside this one on a departure row. Blue is the app's colour for a
    // changed arrangement (docs/style-guide.md, "Semantic roles").
    it('is blue, not the delay badge orange', () => {
      const root = badge(CHANGED);
      expect(colourOf(root)).toMatch(/blue/);
      expect(colourOf(root)).not.toMatch(/orange/);
    });

    it('names the booked platform it changed from once the api compares against it', () => {
      const root = badge({ ...CHANGED, plannedPlatform: '7', bookedPlatform: '6', platformBasis: 'board' });
      expect(spokenText(root)).toBe('Platform 9 (changed from 6)');
    });

    it('is filled blue once confirmed', () => {
      const root = badge({ ...CHANGED, bookedPlatform: '6', platformBasis: 'confirmed' });
      expect(root).toHaveAttribute('data-variant', 'filled');
      expect(root).toHaveAttribute('data-platform-confirmed', 'true');
      expect(colourOf(root)).toMatch(/blue/);
      expect(spokenText(root)).toBe('Platform 9 (confirmed, changed from 6)');
    });
  });

  describe('cancelled', () => {
    it('is a red light badge, the whole label struck through', () => {
      const root = badge({ ...LIVE, platformStatus: 'cancelled' });
      expect(root).toHaveAttribute('data-platform-state', 'cancelled');
      expect(root).toHaveAttribute('data-platform-cancelled', 'true');
      expect(root).toHaveAttribute('data-variant', 'light');
      expect(colourOf(root)).toMatch(/red/);
      expect(screen.getByText('Platform 4').tagName).toBe('S');
    });

    it('says "(cancelled)" to a screen reader and in its tooltip', () => {
      const root = badge({ ...LIVE, platformStatus: 'cancelled' });
      expect(spokenText(root)).toBe('Platform 4 (cancelled)');
      expect(root).toHaveAttribute('title', 'Cancelled: the train no longer calls at this platform');
    });

    it('wins over a change: the train no longer calls at either platform', () => {
      const root = badge({ ...CHANGED, platformStatus: 'cancelled' });
      expect(root).toHaveAttribute('data-platform-state', 'cancelled');
      expect(spokenText(root)).toBe('Platform 9 (cancelled)');
    });

    it('renders an active or unknown status as a normal platform', () => {
      const root = badge({ ...LIVE, platformStatus: 'provisional' as unknown as 'active' });
      expect(root).toHaveAttribute('data-platform-state', 'live');
      expect(root.querySelector('s')).toBeNull();
    });
  });

  describe('compact (a narrow table column)', () => {
    const cases: [name: string, facts: PlatformFacts, short: string, spoken: string][] = [
      ['booked', BOOKED, '4', 'Platform 4 (booked, not confirmed)'],
      ['live', LIVE, '4', 'Platform 4'],
      ['confirmed', { ...LIVE, platformBasis: 'confirmed' }, '4', 'Platform 4 (confirmed)'],
      ['changed', CHANGED, '6 9', 'Platform 9 (changed from 6)'],
      ['cancelled', { ...LIVE, platformStatus: 'cancelled' }, '4', 'Platform 4 (cancelled)'],
    ];

    it.each(cases)(
      '%s: a short phone form, hidden from screen readers, and the same spoken words',
      (_, facts, short, spoken) => {
        const root = badge(facts, true);
        const compact = root.querySelector('.phoneLabel__compact');
        expect(compact).toHaveAttribute('aria-hidden', 'true');
        expect(compact).toHaveTextContent(short);
        expect(spokenText(root)).toBe(spoken);
      },
    );

    it('keeps the look in the short form: dotted, struck old number, struck cancelled', () => {
      expect(badge(BOOKED, true).querySelector('.phoneLabel__compact .platformBadge__booked')).not.toBeNull();
    });

    it('strikes the old number in the short form of a change', () => {
      expect(badge(CHANGED, true).querySelector('.phoneLabel__compact s')).toHaveTextContent('6');
    });

    it('strikes the short form of a cancelled platform', () => {
      expect(
        badge({ ...LIVE, platformStatus: 'cancelled' }, true).querySelector('.phoneLabel__compact s'),
      ).toHaveTextContent('4');
    });
  });

  it('is sentence case and never cut short (`data-platform-badge`, globals.css)', () => {
    badge(LIVE);
    expect(badgeTextTransform(screen.getByText('Platform 4'))).toBe('none');
  });
});
