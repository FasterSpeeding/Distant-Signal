import { describe, it, expect } from 'vitest';
import { screen, fireEvent, createEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LineDefinitionTooltip } from './LineDefinitionTooltip';

describe('LineDefinitionTooltip', () => {
  it('renders a trigger icon with an accessible label', () => {
    renderWithMantine(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
    expect(screen.getByLabelText('How this line is defined')).toBeInTheDocument();
  });

  it('renders a real SVG icon rather than the literal "ⓘ" glyph', () => {
    renderWithMantine(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
    const trigger = screen.getByLabelText('How this line is defined');
    expect(trigger.textContent).not.toContain('ⓘ');
    expect(trigger.querySelector('svg')).toBeInTheDocument();
  });

  it('shows the stations and operators in the tooltip content on hover', async () => {
    renderWithMantine(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
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

  it('shows the tooltip content on a touch tap, not just mouse hover', async () => {
    // See LastUpdated.test.tsx's equivalent test for why a touch pointer
    // must be established with pointerdown first (and why it's done via a
    // manually tagged event rather than
    // `fireEvent.pointerDown(el, { pointerType })` -- jsdom has no
    // PointerEvent constructor).
    renderWithMantine(<LineDefinitionTooltip stations={['WOK', 'WAT']} operators={['SW']} />);
    const trigger = screen.getByLabelText('How this line is defined');
    const pointerDown = createEvent.pointerDown(trigger);
    Object.defineProperty(pointerDown, 'pointerType', { value: 'touch' });
    fireEvent(trigger, pointerDown);
    fireEvent.mouseEnter(trigger);
    expect(await screen.findByText('Stations: WOK, WAT')).toBeInTheDocument();
  });

  it('opens downward, into the page, rather than up over the nav', async () => {
    // The trigger sits directly under the header; Mantine's default
    // `position="top"` put the tooltip on top of the site title on tablet
    // and over the whole nav on mobile.
    //
    // This Mantine version (9.4.1) renders no `data-position` attribute on
    // `Tooltip` — placement is only expressed through the floating offset
    // floating-ui applies. A `top` position pushes the tooltip *up* (a
    // negative `top` relative to the reference); `bottom` pushes it *down*
    // (positive). Asserting the sign of that offset is the closest
    // available proxy for "opens downward" in this jsdom environment,
    // which never computes real element sizes/positions.
    renderWithMantine(<LineDefinitionTooltip stations={['WOK']} operators={['SW']} />);
    fireEvent.mouseEnter(screen.getByLabelText('How this line is defined'));
    const tooltip = await screen.findByRole('tooltip');
    expect(parseFloat(tooltip.style.top)).toBeGreaterThan(0);
  });
});
