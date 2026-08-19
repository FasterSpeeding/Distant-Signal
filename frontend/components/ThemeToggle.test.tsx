import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { ThemeToggle } from './ThemeToggle';

function renderWithProvider() {
  return render(
    <MantineProvider theme={theme} defaultColorScheme="auto">
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
    // Full circle: the auto marker (see the test below) must reappear once
    // the cycle returns to "auto", not just disappear once and stay gone.
    expect(screen.getByText('A')).toBeInTheDocument();
  });

  it('marks the auto state with a visible indicator, distinct from the sun/moon icon', () => {
    renderWithProvider();
    // "auto" resolves to light here (system preference is polyfilled to
    // light), so the icon alone is ☀️ - identical to what explicit "light"
    // will render next. Without a separate marker, clicking away from
    // "auto" to "light" would change nothing the user can see.
    expect(screen.getByText('A')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button')); // -> light
    // Same resolved icon as before, but the auto marker must be gone now
    // that a scheme is explicitly selected - this is the visible change
    // click 1 must produce.
    expect(screen.queryByText('A')).not.toBeInTheDocument();
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
      <MantineProvider theme={theme} defaultColorScheme="auto">
        <ThemeToggle />
      </MantineProvider>,
    );

    expect(html).toContain('Theme: auto. Click to switch.');
    expect(html).not.toContain('Theme: dark. Click to switch.');
    expect(html).toContain('☀️');
    expect(html).not.toContain('🌙');
  });

  it('keeps the auto marker out of the accessibility tree', () => {
    renderWithProvider();
    // The button's own `aria-label` already says "Theme: auto", so an
    // exposed "A" next to it is just a bare, meaningless letter to a
    // screen reader. It stays visible; it's only hidden from AT.
    const marker = screen.getByText('A');
    expect(marker).toHaveAttribute('aria-hidden', 'true');
  });
});
