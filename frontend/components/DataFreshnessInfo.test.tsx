import { describe, it, expect } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DataFreshnessInfo } from './DataFreshnessInfo';
import type { DataFreshness } from '@/lib/types';

const freshness: DataFreshness = {
  stations: '2026-07-15T09:00:00Z',
  tocs: '2026-07-15T08:00:00Z',
  incidents: null,
};

describe('DataFreshnessInfo', () => {
  it('renders an info icon', () => {
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    expect(screen.getByRole('button', { name: 'Data freshness' })).toBeInTheDocument();
  });

  it('renders a real SVG icon rather than the literal "ⓘ" glyph', () => {
    // The glyph hits an emoji/font fallback and renders as a broken-looking
    // box in most environments. It must be a drawn icon, not a character.
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    const button = screen.getByRole('button', { name: 'Data freshness' });
    expect(button.textContent).not.toContain('ⓘ');
    expect(button.querySelector('svg')).toBeInTheDocument();
  });

  it('shows a last-updated row for each present timestamp', async () => {
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    // Mantine's Tooltip doesn't mount its floating content into the DOM
    // at all until actually triggered — hover it first (same pattern as
    // LineDefinitionTooltip.test.tsx).
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^Stations:/)).toBeInTheDocument();
    expect(screen.getByText(/^TOCs:/)).toBeInTheDocument();
  });

  it('shows "never fetched" for a null timestamp', async () => {
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^Incidents: never fetched/)).toBeInTheDocument();
  });
});
