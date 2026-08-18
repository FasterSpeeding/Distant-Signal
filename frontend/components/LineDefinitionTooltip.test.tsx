import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { LineDefinitionTooltip } from './LineDefinitionTooltip';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider theme={theme}>{ui}</MantineProvider>);
}

describe('LineDefinitionTooltip', () => {
  it('renders a trigger icon with an accessible label', () => {
    renderWithProvider(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
    expect(screen.getByLabelText('How this line is defined')).toBeInTheDocument();
  });

  it('renders a real SVG icon rather than the literal "ⓘ" glyph', () => {
    renderWithProvider(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
    const trigger = screen.getByLabelText('How this line is defined');
    expect(trigger.textContent).not.toContain('ⓘ');
    expect(trigger.querySelector('svg')).toBeInTheDocument();
  });

  it('shows the stations and operators in the tooltip content on hover', async () => {
    renderWithProvider(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
    // Unlike the Combobox-based dropdowns elsewhere in this codebase
    // (which pre-render their options, just hidden via CSS until
    // positioned), Mantine's Tooltip doesn't mount its content into the
    // DOM at all until actually triggered — hover it first.
    fireEvent.mouseEnter(screen.getByLabelText('How this line is defined'));

    // No `hidden` option here: it's `getByRole`-only (`@testing-library
    // /dom`'s `SelectorMatcherOptions` has no such field), so passing it
    // was both a type error and inert at runtime.
    expect(await screen.findByText('Stations: WOK, WAT')).toBeInTheDocument();
    expect(screen.getByText('Operators: SW')).toBeInTheDocument();
  });
});
