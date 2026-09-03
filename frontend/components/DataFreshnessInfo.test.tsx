import { describe, it, expect } from 'vitest';
import { screen, fireEvent, createEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DataFreshnessInfo } from './DataFreshnessInfo';
import type { DataFreshness } from '@/lib/types';

const freshness: DataFreshness = {
  stations: '2026-07-15T09:00:00Z',
  tocs: '2026-07-15T08:00:00Z',
  incidents: null,
  tfl: '2026-08-22T03:00:00Z',
  schedule_feed: '2026-08-30T04:00:00Z',
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

  it('shows a row for the TfL feed', async () => {
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^TfL:/)).toBeInTheDocument();
  });

  it('shows a row for the schedule feed', async () => {
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^Schedule feed:/)).toBeInTheDocument();
  });

  it('shows "never fetched" for a null schedule feed timestamp', async () => {
    renderWithMantine(<DataFreshnessInfo freshness={{ ...freshness, schedule_feed: null }} />);
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^Schedule feed: never fetched/)).toBeInTheDocument();
  });

  it('shows the freshness rows on a touch tap, not just mouse hover', async () => {
    // See the equivalent LastUpdated test for why a touch pointer must be
    // established with pointerdown first (and why it's done via a manually
    // tagged event rather than `fireEvent.pointerDown(el, { pointerType })`
    // -- jsdom has no PointerEvent constructor).
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    const button = screen.getByRole('button', { name: 'Data freshness' });
    const pointerDown = createEvent.pointerDown(button);
    Object.defineProperty(pointerDown, 'pointerType', { value: 'touch' });
    fireEvent(button, pointerDown);
    fireEvent.mouseEnter(button);
    expect(await screen.findByText(/^Stations:/)).toBeInTheDocument();
  });
});
