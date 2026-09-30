import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { visibleText } from '@/test/routeText';
import { routeLabel, spokenRoute } from '@/lib/stationLabel';
import { RouteArrow, RouteText } from './RouteArrow';

/** The text a screen reader gets: every aria-hidden subtree removed, the
 * visually hidden "to" kept. */
function spokenText(el: Element): string {
  const clone = el.cloneNode(true) as Element;
  clone.querySelectorAll('[aria-hidden="true"]').forEach((node) => node.remove());
  return (clone.textContent ?? '').replace(/\s+/g, ' ').trim();
}

describe('RouteArrow', () => {
  it('shows the arrow but reads as "to"', () => {
    renderWithMantine(
      <p data-testid="route">
        KGX <RouteArrow /> YRK
      </p>,
    );
    const route = screen.getByTestId('route');
    expect(visibleText(route)).toBe('KGX → YRK');
    expect(spokenText(route)).toBe('KGX to YRK');
    expect(route.querySelector('[aria-hidden="true"]')).toHaveTextContent('→');
  });

  it('gives a link the accessible name "KGX to YRK"', () => {
    renderWithMantine(
      <a href="/x">
        KGX <RouteArrow /> YRK
      </a>,
    );
    expect(screen.getByRole('link', { name: 'KGX to YRK' })).toBeInTheDocument();
  });
});

describe('RouteText', () => {
  it('swaps the arrow in a route string, leaving the visible text unchanged', () => {
    const label = `${routeLabel('KGX', 'London Kings Cross', 'YRK', 'York')}, 22 Sept 2026`;
    renderWithMantine(
      <p data-testid="route">
        <RouteText>{label}</RouteText>
      </p>,
    );
    const route = screen.getByTestId('route');
    expect(visibleText(route)).toBe('London Kings Cross (KGX) → York (YRK), 22 Sept 2026');
    expect(spokenText(route)).toBe('London Kings Cross (KGX) to York (YRK), 22 Sept 2026');
  });

  it('handles several arrows, and none', () => {
    const { rerender } = renderWithMantine(
      <p data-testid="route">
        <RouteText>{'PAD → RDG → BRI'}</RouteText>
      </p>,
    );
    expect(visibleText(screen.getByTestId('route'))).toBe('PAD → RDG → BRI');
    expect(spokenText(screen.getByTestId('route'))).toBe('PAD to RDG to BRI');
    rerender(
      <p data-testid="route">
        <RouteText>Untitled journey</RouteText>
      </p>,
    );
    expect(screen.getByTestId('route')).toHaveTextContent(/^Untitled journey$/);
  });
});

describe('spokenRoute', () => {
  it('spells the arrow out as "to" for plain-text contexts', () => {
    expect(spokenRoute('KGX → YRK · 16:00')).toBe('KGX to YRK · 16:00');
    expect(spokenRoute('dep. BTH 10:32 → arr. SWI 11:08')).toBe('dep. BTH 10:32 to arr. SWI 11:08');
    expect(spokenRoute('No arrow here')).toBe('No arrow here');
  });
});
