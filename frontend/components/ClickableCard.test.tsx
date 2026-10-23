import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fireEvent, screen } from '@testing-library/react';
import { Button, Radio, Text } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { TextLink } from './TextLink';
import {
  CARD_PRIMARY,
  CardTitleButton,
  CardTitleLink,
  ClickableCard,
  selectableCardRadioProps,
  StaticCard,
} from './ClickableCard';

const css = readFileSync(join(__dirname, '..', 'app', 'globals.css'), 'utf8');

describe('ClickableCard with a CardTitleLink', () => {
  it('is a raised card holding one link named by the title alone', () => {
    const { container } = renderWithMantine(
      <ClickableCard>
        <Text fw={500}>
          <CardTitleLink href="/journeys/7">Woking to York, 17 Sep 2026</CardTitleLink>
        </Text>
        <Text size="sm">Departs 18:00 · 2 legs</Text>
      </ClickableCard>,
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAccessibleName('Woking to York, 17 Sep 2026');
    expect(link).toHaveAttribute('href', '/journeys/7');
    expect(link).toHaveAttribute('data-card-primary');
    const card = container.querySelector('[data-clickable-card]');
    expect(card).not.toBeNull();
    expect(card).toContainElement(link);
    // The rest of the card's text is not part of the link's name.
    expect(link).not.toHaveTextContent('Departs');
    expect(card).toHaveAttribute('data-with-border', 'true');
    expect(card).toHaveStyle({ '--paper-shadow': 'var(--mantine-shadow-sm)' });
  });

  it('takes an aria-label when the title alone does not say where it goes', () => {
    renderWithMantine(
      <ClickableCard>
        <CardTitleLink href="/lines?status=severe" ariaLabel="3 lines with severe disruption, view in Lines">
          Severe
        </CardTitleLink>
      </ClickableCard>,
    );
    expect(screen.getByRole('link')).toHaveAccessibleName('3 lines with severe disruption, view in Lines');
  });

  it('keeps an inner control its own, never nested in the link', () => {
    const onFollow = vi.fn();
    renderWithMantine(
      <ClickableCard>
        <CardTitleLink href="/journeys/7">Woking to York</CardTitleLink>
        <Button onClick={onFollow}>Follow</Button>
      </ClickableCard>,
    );
    const link = screen.getByRole('link', { name: 'Woking to York' });
    const follow = screen.getByRole('button', { name: 'Follow' });
    expect(link).not.toContainElement(follow);
    expect(follow).not.toContainElement(link);
    fireEvent.click(follow);
    expect(onFollow).toHaveBeenCalledTimes(1);
  });

  it('passes padding, fill, className and data attributes to the card', () => {
    const { container } = renderWithMantine(
      <ClickableCard padding="lg" fill className="currentJourneyCard" data-suggestion="s1">
        <CardTitleLink href="/x">X</CardTitleLink>
      </ClickableCard>,
    );
    const card = container.querySelector('[data-clickable-card]');
    expect(card).toHaveStyle({ '--card-padding': 'var(--mantine-spacing-lg)' });
    expect(card).toHaveClass('ds-column');
    expect(card).toHaveClass('currentJourneyCard');
    expect(card).toHaveAttribute('data-suggestion', 's1');
  });
});

describe('ClickableCard with a CardTitleButton', () => {
  it('is a native button named by the title, and runs its action', () => {
    const onSelect = vi.fn();
    renderWithMantine(
      <ClickableCard>
        <CardTitleButton onClick={onSelect}>18:02 · York (YRK)</CardTitleButton>
      </ClickableCard>,
    );
    const button = screen.getByRole('button');
    expect(button.tagName).toBe('BUTTON');
    expect(button).toHaveAttribute('type', 'button');
    expect(button).toHaveAccessibleName('18:02 · York (YRK)');
    fireEvent.click(button);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it('does not run the card action when an inner link is used', () => {
    const onSelect = vi.fn();
    renderWithMantine(
      <ClickableCard>
        <CardTitleButton onClick={onSelect}>18:02 · York (YRK)</CardTitleButton>
        <TextLink href="/train/W1/2026-09-17">View live status</TextLink>
      </ClickableCard>,
    );
    fireEvent.click(screen.getByRole('link', { name: 'View live status' }));
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('can make a visible Mantine Button the target', () => {
    renderWithMantine(
      <ClickableCard>
        <Text>10:04 York to Edinburgh</Text>
        <Button {...CARD_PRIMARY}>Take this one</Button>
      </ClickableCard>,
    );
    expect(screen.getByRole('button', { name: 'Take this one' })).toHaveAttribute('data-card-primary');
  });
});

describe('a selectable ClickableCard', () => {
  it('makes the radio the target and stretches its label', () => {
    const { container } = renderWithMantine(
      <ClickableCard selected>
        <Radio {...selectableCardRadioProps} checked onChange={() => undefined} label="Woking to York, direct" />
      </ClickableCard>,
    );
    const radio = screen.getByRole('radio', { name: 'Woking to York, direct' });
    expect(radio).toHaveAttribute('data-card-primary');
    expect(container.querySelector('label.ds-card-stretch')).not.toBeNull();
    expect(container.querySelector('[data-clickable-card]')).toHaveAttribute('data-selected');
  });
});

describe('StaticCard', () => {
  it('is the same box, flat and not clickable', () => {
    const { container } = renderWithMantine(<StaticCard padding="xs">Cancelled</StaticCard>);
    const card = container.querySelector('.mantine-Card-root');
    expect(card).toHaveAttribute('data-with-border', 'true');
    expect(card).not.toHaveAttribute('data-clickable-card');
    expect(card).not.toHaveStyle({ '--paper-shadow': 'var(--mantine-shadow-sm)' });
  });
});

// The look lives in the stylesheet, not inline styles (docs/style-guide.md,
// "Code conventions"): check the rules the pattern relies on are there.
describe('clickable card rules in globals.css', () => {
  it('stretches the target over the card and lifts inner controls above it', () => {
    expect(css).toMatch(
      /\[data-card-primary\]::after,\s*\[data-clickable-card\] \.ds-card-stretch::after \{\s*content: '';\s*position: absolute;\s*inset: 0;/,
    );
    expect(css).toMatch(
      /\[data-clickable-card\]\s*:is\(a, button, input, select, textarea, summary, \[role='button'\]/,
    );
    expect(css).toMatch(/:not\(\s*\[data-card-primary\]\s*\) \{\s*position: relative;\s*z-index: 1;/);
  });

  it('has the pointer, hover, pressed and focus states', () => {
    expect(css).toMatch(/\[data-clickable-card\] \{[^}]*cursor: pointer;/);
    expect(css).toMatch(/\[data-clickable-card\]:hover \{[^}]*border-color: var\(--mantine-color-anchor\);/);
    expect(css).toContain('[data-clickable-card]:has([data-card-primary]:active, .ds-card-stretch:active)');
    expect(css).toMatch(
      /\[data-clickable-card\]:has\(\[data-card-primary\]:focus-visible\) \{[^}]*outline: 2px solid var\(--mantine-color-anchor\);\s*outline-offset: 2px;/,
    );
  });

  it('only animates under prefers-reduced-motion: no-preference', () => {
    const transition = css.indexOf('transition:\n      border-color 120ms ease');
    expect(transition).toBeGreaterThan(-1);
    const media = css.lastIndexOf('@media (prefers-reduced-motion: no-preference)', transition);
    expect(media).toBeGreaterThan(-1);
    expect(css.slice(media, transition)).toMatch(
      /^@media \(prefers-reduced-motion: no-preference\) \{\s*\[data-clickable-card\] \{\s*$/,
    );
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: no-preference\) \{\s*\[data-clickable-card\]:has\(\[data-card-primary\]:active, \.ds-card-stretch:active\) \{\s*transform: translateY\(1px\);/,
    );
  });
});
