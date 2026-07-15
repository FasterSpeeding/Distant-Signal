import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { ThemeToggle } from './ThemeToggle';

function renderWithProvider() {
  return render(
    <MantineProvider defaultColorScheme="auto">
      <ThemeToggle />
    </MantineProvider>,
  );
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

  it('shows the sun icon when resolved to light, moon when resolved to dark', () => {
    renderWithProvider();
    const button = screen.getByRole('button');
    // matchMedia is polyfilled (vitest.setup.ts) to always report no dark
    // preference, so "auto" resolves to light here.
    expect(button).toHaveTextContent('☀️');

    fireEvent.click(button); // -> light
    expect(button).toHaveTextContent('☀️');

    fireEvent.click(button); // -> dark
    expect(button).toHaveTextContent('🌙');
  });

  it('server-rendered output ignores localStorage, avoiding a hydration mismatch', () => {
    // A returning visitor has "dark" persisted from a prior session.
    // `renderToString` never runs effects, so this simulates exactly what
    // the server sends down: it must match what the client's first
    // (pre-mount) render produces, regardless of what's in localStorage,
    // or React discards the SSR-ed tree on hydration.
    localStorage.setItem('mantine-color-scheme-value', 'dark');

    const html = renderToString(
      <MantineProvider defaultColorScheme="auto">
        <ThemeToggle />
      </MantineProvider>,
    );

    expect(html).toContain('Theme: auto. Click to switch.');
    expect(html).not.toContain('Theme: dark. Click to switch.');
    expect(html).toContain('☀️');
    expect(html).not.toContain('🌙');
  });
});
