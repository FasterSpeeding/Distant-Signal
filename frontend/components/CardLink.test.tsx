import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CardLink } from './CardLink';
import { ButtonLink } from './ButtonLink';

describe('CardLink', () => {
  it('is one link around a bordered, shadowed card (tokens.card.link)', () => {
    renderWithMantine(
      <CardLink href="/lines/x" ariaLabel="Line X, view details">
        Line X
      </CardLink>,
    );
    const link = screen.getByRole('link', { name: 'Line X, view details' });
    expect(link).toHaveAttribute('href', '/lines/x');
    expect(link).toHaveAttribute('data-card-link');
    const card = link.querySelector('[data-card]');
    expect(card).not.toBeNull();
    expect(card).toHaveAttribute('data-with-border', 'true');
    expect(card).toHaveStyle({ '--paper-shadow': 'var(--mantine-shadow-sm)' });
    expect(link).not.toHaveAttribute('style');
  });

  it('passes padding through to the card', () => {
    renderWithMantine(
      <CardLink href="/status" padding="lg">
        Tile
      </CardLink>,
    );
    expect(screen.getByRole('link').querySelector('[data-card]')).toHaveStyle({
      '--card-padding': 'var(--mantine-spacing-lg)',
    });
  });

  // The look lives in the stylesheet, not inline styles (docs/style-guide.md,
  // "Code conventions"): check the rules CardLink relies on are there.
  it('has its colour, underline, hover and focus rules in globals.css', () => {
    const css = readFileSync(join(__dirname, '..', 'app', 'globals.css'), 'utf8');
    expect(css).toMatch(/a\[data-card-link\] \{\s*display: block;\s*color: inherit;\s*text-decoration: none;/);
    expect(css).toContain('a[data-card-link]:hover [data-card]');
    expect(css).toContain('a[data-card-link]:focus-visible [data-card]');
  });
});

describe('ButtonLink', () => {
  it('renders one <a> drawn as a button, not a <button> inside a link', () => {
    renderWithMantine(
      <ButtonLink href="/groups/new" variant="default" size="xs">
        Create a group
      </ButtonLink>,
    );
    const link = screen.getByRole('link', { name: 'Create a group' });
    expect(link).toHaveAttribute('href', '/groups/new');
    expect(link).toHaveClass('mantine-Button-root');
    expect(link).toHaveAttribute('data-variant', 'default');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});
