import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { renderWithMantine } from '@/test/render';
import { ThemeToggle } from './ThemeToggle';

function renderWithProvider() {
  return renderWithMantine(<ThemeToggle />, { defaultColorScheme: 'auto' });
}

describe('ThemeToggle', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('starts on auto (the default) with a label stating so', () => {
    renderWithProvider();
    expect(screen.getByLabelText('Theme: auto. Click to switch.')).toBeInTheDocument();
  });

  it('cycles auto -> light -> dark -> auto on repeated clicks', () => {
    renderWithProvider();
    const button = screen.getByRole('button');

    fireEvent.click(button);
    expect(screen.getByLabelText('Theme: light. Click to switch.')).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.getByLabelText('Theme: dark. Click to switch.')).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.getByLabelText('Theme: auto. Click to switch.')).toBeInTheDocument();
  });

  it('shows the sun-moon composite icon when in auto mode, changing to sun-only on light', () => {
    renderWithProvider();
    const button = screen.getByRole('button');
    const { container } = renderWithMantine(<ThemeToggle />, { defaultColorScheme: 'auto' });

    // In auto mode (resolved to light), renders the composite sun-moon icon
    const svgs = container.querySelectorAll('svg');
    // IconSunMoon has both sun rays and a moon path in one SVG
    expect(svgs.length).toBeGreaterThan(0);

    fireEvent.click(button); // -> light
    // Explicit light mode should have a different icon now
    expect(screen.getByLabelText('Theme: light. Click to switch.')).toBeInTheDocument();
  });

  it('shows SVG icons: sun-moon when auto, sun when light, moon when dark', () => {
    const { container } = renderWithMantine(<ThemeToggle />, { defaultColorScheme: 'auto' });
    const button = screen.getByRole('button');

    // Start: auto resolves to light, so shows sun-moon icon
    let svgs = container.querySelectorAll('svg');
    expect(svgs.length).toBe(1);
    const sunMoonSvg = svgs[0];
    expect(sunMoonSvg.querySelector('circle')).toBeInTheDocument(); // sun circle
    expect(sunMoonSvg.querySelector('path')).toBeInTheDocument(); // moon path

    fireEvent.click(button); // -> light
    // Light mode: sun icon only (circle + rays)
    svgs = container.querySelectorAll('svg');
    expect(svgs.length).toBe(1);
    const sunSvg = svgs[0];
    expect(sunSvg.querySelector('circle')).toBeInTheDocument();
    // Sun has lines, moon has a path
    const lines = sunSvg.querySelectorAll('line');
    expect(lines.length).toBeGreaterThan(0);

    fireEvent.click(button); // -> dark
    // Dark mode: moon icon only
    svgs = container.querySelectorAll('svg');
    expect(svgs.length).toBe(1);
    const moonSvg = svgs[0];
    expect(moonSvg.querySelector('path')).toBeInTheDocument();
  });

  it('server-rendered output ignores localStorage, avoiding a hydration mismatch', () => {
    // A returning visitor has "dark" persisted from a prior session.
    // `renderToString` never runs effects, so this simulates exactly what
    // the server sends down: it must match what the client's first
    // (pre-mount) render produces, regardless of what's in localStorage,
    // or React discards the SSR-ed tree on hydration.
    localStorage.setItem('mantine-color-scheme-value', 'dark');

    const html = renderToString(
      <MantineProvider theme={theme} defaultColorScheme="auto">
        <ThemeToggle />
      </MantineProvider>,
    );

    expect(html).toContain('Theme: auto. Click to switch.');
    expect(html).not.toContain('Theme: dark. Click to switch.');
    // Should contain SVG markup, not emoji
    expect(html).toContain('<svg');
    expect(html).not.toContain('☀️');
    expect(html).not.toContain('🌙');
  });

  it('contains no emoji characters in rendered output', () => {
    const { container } = renderWithMantine(<ThemeToggle />, { defaultColorScheme: 'auto' });
    // Verify no emoji characters in the DOM
    const html = container.innerHTML;
    expect(html).not.toContain('☀️');
    expect(html).not.toContain('🌙');
    // Verify SVG icons are present
    expect(container.querySelector('svg')).toBeInTheDocument();
  });
});
