import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { TextLink } from './TextLink';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider theme={theme}>{ui}</MantineProvider>);
}

describe('TextLink', () => {
  it('renders an anchor to the href carrying the link colour', () => {
    renderWithProvider(<TextLink href="/lines">All Lines</TextLink>);

    const link = screen.getByRole('link', { name: 'All Lines' });
    expect(link).toHaveAttribute('href', '/lines');
    // The seven hand-rolled call sites this replaces all set this colour
    // directly; the rendered result must not change.
    expect(screen.getByText('All Lines')).toHaveStyle({ color: 'var(--mantine-color-anchor)' });
  });

  it('defaults to an underline on hover and focus only', () => {
    renderWithProvider(<TextLink href="/lines">All Lines</TextLink>);
    // jsdom applies no stylesheet, so the hook the rules in globals.css
    // hang off is what's assertable here (globals.test.ts asserts the
    // rules themselves).
    expect(screen.getByRole('link', { name: 'All Lines' })).toHaveAttribute('data-text-link', 'hover');
  });

  it('can opt into an always-on underline for links sitting among body text', () => {
    renderWithProvider(
      <TextLink href="/lines/wcml" underline="always">
        Back to line
      </TextLink>,
    );

    expect(screen.getByRole('link', { name: 'Back to line' })).toHaveAttribute('data-text-link', 'always');
  });
});
