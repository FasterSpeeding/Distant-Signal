import { describe, it, expect } from 'vitest';
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { Tooltip } from '@mantine/core';
import { renderWithMantine } from '@/test/render';

// Every Mantine `Tooltip` in the app, as the theme configures it
// (`lib/theme.ts`, `components.Tooltip.defaultProps`): no `events` prop here,
// so this is the default a new tooltip gets. A keyboard user has to be able
// to open it, and a screen reader has to be told about it: Mantine (via
// floating-ui's `useRole({ role: 'tooltip' })`) gives the open tooltip
// `role="tooltip"` and an id, and points the trigger's `aria-describedby`
// at it, but only while it is open, and by default only hover opened it.
describe('Tooltip (app theme defaults)', () => {
  function renderTooltip() {
    renderWithMantine(
      <Tooltip label="Departs from platform 4">
        <button type="button">Platform</button>
      </Tooltip>,
    );
    return screen.getByRole('button', { name: 'Platform' });
  }

  it('is not in the DOM, or described, until opened', () => {
    const trigger = renderTooltip();
    expect(screen.queryByRole('tooltip')).toBeNull();
    expect(trigger).not.toHaveAttribute('aria-describedby');
  });

  it('opens on keyboard focus and is announced as the trigger’s description', async () => {
    const trigger = renderTooltip();
    // A key press first, as a keyboard user's Tab would be: floating-ui
    // opens on focus only for keyboard (`:focus-visible`) focus, not the
    // focus a mouse click gives.
    fireEvent.keyDown(document.body, { key: 'Tab' });
    act(() => trigger.focus());

    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip).toHaveTextContent('Departs from platform 4');
    expect(tooltip.id).not.toBe('');
    expect(trigger).toHaveAttribute('aria-describedby', tooltip.id);
    expect(trigger).toHaveAccessibleDescription('Departs from platform 4');
  });

  it('still opens on hover', async () => {
    const trigger = renderTooltip();
    fireEvent.mouseEnter(trigger);
    const tooltip = await screen.findByRole('tooltip');
    expect(trigger).toHaveAttribute('aria-describedby', tooltip.id);
  });

  it('closes, and drops the description, when focus leaves', async () => {
    const trigger = renderTooltip();
    fireEvent.keyDown(document.body, { key: 'Tab' });
    act(() => trigger.focus());
    await screen.findByRole('tooltip');
    act(() => trigger.blur());
    // floating-ui closes on blur after a tick, not synchronously.
    await waitFor(() => expect(screen.queryByRole('tooltip')).toBeNull());
    expect(trigger).not.toHaveAttribute('aria-describedby');
  });
});
