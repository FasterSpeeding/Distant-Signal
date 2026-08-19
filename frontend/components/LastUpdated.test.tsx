import { describe, it, expect } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { renderWithMantine } from '@/test/render';
import { LastUpdated } from './LastUpdated';

describe('LastUpdated', () => {
  it('server-rendered output shows a fixed absolute time, never a relative one (avoids hydration mismatch)', () => {
    // Mirrors the ThemeToggle regression test: renderToString never runs
    // effects, so this is exactly what the server sends down. It must not
    // depend on "now" (real wall-clock time at test-run time), or it can
    // never match the client's own pre-mount render.
    const html = renderToString(
      <MantineProvider theme={theme}>
        <LastUpdated timestamp="2026-07-15T09:00:00Z" />
      </MantineProvider>,
    );
    expect(html).toContain('Updated');
    expect(html).not.toMatch(/\d+[mhd] ago|just now/);
  });

  it('shows a relative time once mounted', () => {
    renderWithMantine(<LastUpdated timestamp="2026-07-15T09:00:00Z" />);
    expect(screen.getByText(/Updated (just now|\d+[mhd] ago)/)).toBeInTheDocument();
  });

  it('supports a custom label', () => {
    renderWithMantine(<LastUpdated timestamp="2026-07-15T09:00:00Z" label="Stations:" />);
    expect(screen.getByText(/^Stations:/)).toBeInTheDocument();
  });

  it('shows the exact time in a tooltip on hover by default', async () => {
    renderWithMantine(<LastUpdated timestamp="2026-07-15T09:00:00Z" />);
    // Mantine's Tooltip doesn't mount its floating content into the DOM
    // at all until actually triggered — hover it first (same pattern as
    // LineDefinitionTooltip.test.tsx).
    fireEvent.mouseEnter(screen.getByText(/^Updated/));
    expect(await screen.findByRole('tooltip', { hidden: true })).toBeInTheDocument();
  });

  it('does not show a tooltip on hover when withTooltip is false', () => {
    renderWithMantine(<LastUpdated timestamp="2026-07-15T09:00:00Z" withTooltip={false} />);
    fireEvent.mouseEnter(screen.getByText(/^Updated/));
    expect(screen.queryByRole('tooltip', { hidden: true })).not.toBeInTheDocument();
  });
});
