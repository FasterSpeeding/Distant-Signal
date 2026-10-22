import { afterEach, describe, expect, it, vi } from 'vitest';
import { captureScrollAnchor, restoreScrollAnchor } from './scrollAnchor';

/** A container of `[data-item]` elements whose tops are `tops` (each 100px tall). */
function container(items: [string, number][]): HTMLElement {
  const root = document.createElement('div');
  for (const [value, top] of items) {
    const item = document.createElement('div');
    item.setAttribute('data-item', value);
    item.getBoundingClientRect = () => ({ top, bottom: top + 100 }) as DOMRect;
    root.appendChild(item);
  }
  return root;
}

describe('scroll anchoring', () => {
  afterEach(() => vi.restoreAllMocks());

  it('holds the first item on screen still, not one scrolled past', () => {
    const scrollBy = vi.spyOn(window, 'scrollBy').mockImplementation(() => {});
    // "a" is scrolled out of view above; "b" is the first on screen.
    const anchor = captureScrollAnchor(
      container([
        ['a', -150],
        ['b', -20],
        ['c', 80],
      ]),
      'data-item',
    );
    expect(anchor).toEqual({ attribute: 'data-item', value: 'b', top: -20 });
    // Two cards prepended above: everything moved down 240px.
    const after = container([
      ['y', -150],
      ['z', -50],
      ['a', 90],
      ['b', 220],
      ['c', 320],
    ]);
    expect(restoreScrollAnchor(after, anchor)).toBe(240);
    expect(scrollBy).toHaveBeenCalledWith(0, 240);
  });

  it('does nothing when the browser already kept the item in place', () => {
    const scrollBy = vi.spyOn(window, 'scrollBy').mockImplementation(() => {});
    const anchor = captureScrollAnchor(container([['b', 40]]), 'data-item');
    expect(
      restoreScrollAnchor(
        container([
          ['a', -60],
          ['b', 40],
        ]),
        anchor,
      ),
    ).toBe(0);
    expect(scrollBy).not.toHaveBeenCalled();
  });

  it('falls back to the first item when all are above the viewport, and copes with none', () => {
    expect(
      captureScrollAnchor(
        container([
          ['a', -500],
          ['b', -300],
        ]),
        'data-item',
      )?.value,
    ).toBe('a');
    expect(captureScrollAnchor(container([]), 'data-item')).toBeNull();
    expect(captureScrollAnchor(null, 'data-item')).toBeNull();
    expect(restoreScrollAnchor(container([['b', 0]]), { attribute: 'data-item', value: 'gone', top: 0 })).toBe(0);
  });
});
