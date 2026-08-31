import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TextLink } from './TextLink';

describe('TextLink', () => {
  it('renders an anchor to the href carrying the link colour', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);

    const link = screen.getByRole('link', { name: 'All Lines' });
    expect(link).toHaveAttribute('href', '/lines');
    // The seven hand-rolled call sites this replaces all set this colour
    // directly; the rendered result must not change.
    expect(screen.getByText('All Lines')).toHaveStyle({ color: 'var(--mantine-color-anchor)' });
  });

  it('defaults to an underline on hover and focus only', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);
    // jsdom applies no stylesheet, so the hook the rules in globals.css
    // hang off is what's assertable here (globals.test.ts asserts the
    // rules themselves).
    expect(screen.getByRole('link', { name: 'All Lines' })).toHaveAttribute('data-text-link', 'hover');
  });

  it('can opt into an always-on underline for links sitting among body text', () => {
    renderWithMantine(
      <TextLink href="/lines/wcml" underline="always">
        Back to line
      </TextLink>,
    );

    expect(screen.getByRole('link', { name: 'Back to line' })).toHaveAttribute('data-text-link', 'always');
  });

  it('renders no target/rel by default (regression: every existing call site omits them)', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);
    const link = screen.getByRole('link', { name: 'All Lines' });
    expect(link).not.toHaveAttribute('target');
    expect(link).not.toHaveAttribute('rel');
  });

  it('can opt into target/rel for an external link', () => {
    renderWithMantine(
      <TextLink href="https://example.com" target="_blank" rel="noopener noreferrer">
        External
      </TextLink>,
    );
    const link = screen.getByRole('link', { name: 'External' });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  });
});
