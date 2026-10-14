import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { renderWithMantine } from '@/test/render';
import { ThemePreference } from './ThemePreference';

describe('ThemePreference', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('offers match device, light and dark under one label, starting on match device', () => {
    renderWithMantine(<ThemePreference />, { defaultColorScheme: 'auto' });
    expect(screen.getByRole('radiogroup', { name: 'Colour theme' })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Match device' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'Light' })).not.toBeChecked();
    expect(screen.getByRole('radio', { name: 'Dark' })).not.toBeChecked();
  });

  it('saves the chosen scheme', () => {
    renderWithMantine(<ThemePreference />, { defaultColorScheme: 'auto' });
    fireEvent.click(screen.getByRole('radio', { name: 'Dark' }));
    expect(screen.getByRole('radio', { name: 'Dark' })).toBeChecked();
    expect(localStorage.getItem('mantine-color-scheme-value')).toBe('dark');
    fireEvent.click(screen.getByRole('radio', { name: 'Light' }));
    expect(localStorage.getItem('mantine-color-scheme-value')).toBe('light');
  });

  it('server-renders match device whatever is saved, so hydration matches', () => {
    localStorage.setItem('mantine-color-scheme-value', 'dark');
    const html = renderToString(
      <MantineProvider theme={theme} defaultColorScheme="auto">
        <ThemePreference />
      </MantineProvider>,
    );
    const checked = /<input[^>]*checked[^>]*>/.exec(html);
    expect(checked?.[0]).toContain('value="auto"');
  });
});
