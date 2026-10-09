import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NotFound, { metadata } from './not-found';

describe('app-wide not-found page', () => {
  it('renders one h1 and links back into the app', () => {
    renderWithMantine(<NotFound />);
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1);
    expect(screen.getByRole('heading', { level: 1, name: 'Page not found' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Go to the home page' })).toHaveAttribute('href', '/');
    expect(screen.getByRole('link', { name: 'Browse all lines' })).toHaveAttribute('href', '/lines');
    expect(screen.getByRole('link', { name: 'Look up a station' })).toHaveAttribute('href', '/stations');
  });

  it('names the tab and keeps the page out of search indexes', () => {
    expect(metadata.title).toBe('Page not found');
    expect(metadata.robots).toEqual({ index: false });
  });
});
