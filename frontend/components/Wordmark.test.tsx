import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { SignalMark, Wordmark } from './Wordmark';
import { SIGNAL_MARK, signalArmBounds, signalMarkBounds, signalMarkSvg } from '@/lib/brand';

describe('Wordmark', () => {
  it('reads as the site name alone, with the mark hidden from assistive tech', () => {
    const { container } = renderWithMantine(
      <h2>
        <Wordmark />
      </h2>,
    );
    expect(screen.getByRole('heading')).toHaveAccessibleName('Distant Signal');
    expect(container.querySelector('svg')).toHaveAttribute('aria-hidden', 'true');
  });

  it('keeps the pride hook on the name, not the mark', () => {
    const { container } = renderWithMantine(<Wordmark />);
    const title = container.querySelector('[data-site-title]');
    expect(title).toHaveTextContent('Distant Signal');
    expect(title?.querySelector('svg')).toBeNull();
  });

  it('gives each mark its own gradient id', () => {
    const { container } = renderWithMantine(
      <>
        <SignalMark />
        <SignalMark />
      </>,
    );
    const ids = [...container.querySelectorAll('linearGradient')].map((gradient) => gradient.id);
    expect(new Set(ids).size).toBe(2);
  });
});

describe('the signal mark geometry', () => {
  it('matches the favicon, so the mark and the tab icon are one shape', () => {
    const favicon = readFileSync('app/icon.svg', 'utf8');
    expect(favicon).toContain(`points="${SIGNAL_MARK.arm.points}"`);
    expect(favicon).toContain(`transform="${SIGNAL_MARK.arm.transform}"`);
    expect(favicon).toContain(`fill="${SIGNAL_MARK.arm.fill}"`);
    for (const colour of SIGNAL_MARK.tileGradient) expect(favicon).toContain(colour);
  });

  it('renders as a standalone SVG document', () => {
    const svg = signalMarkSvg();
    expect(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg"')).toBe(true);
    expect(svg).toContain(SIGNAL_MARK.arm.points);
    expect(new DOMParser().parseFromString(svg, 'image/svg+xml').querySelector('parsererror')).toBeNull();
  });

  it('turns the arm by the pivot and angle its SVG transform names', () => {
    const { pivot, angle, transform } = SIGNAL_MARK.arm;
    expect(transform).toBe(`translate(${pivot.x},${pivot.y}) rotate(${angle})`);
  });

  it('bounds the raised arm by its fishtail tip, above the post, inside the tile', () => {
    const arm = signalArmBounds();
    const mark = signalMarkBounds();
    const { post, tile } = SIGNAL_MARK;
    // The upper fishtail point, (10.5, -3.5) turned 32 degrees up about (16.4, 12).
    expect(arm.y).toBeCloseTo(3.47, 2);
    expect(arm.y).toBeLessThan(post.y);
    expect(mark.y).toBe(arm.y);
    expect(mark.x).toBe(post.x);
    expect(mark.y + mark.height).toBe(post.y + post.height);
    // So the favicon, the PWA icons and the nav mark, which draw the whole tile, show all of it.
    expect(mark.x).toBeGreaterThan(tile.x);
    expect(mark.y).toBeGreaterThan(tile.y);
    expect(mark.x + mark.width).toBeLessThan(tile.x + tile.size);
    expect(mark.y + mark.height).toBeLessThan(tile.y + tile.size);
  });
});
