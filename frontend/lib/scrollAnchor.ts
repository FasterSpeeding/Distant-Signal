/** Keeps the page from jumping when content is inserted above what the
 * visitor is looking at (the planner's "Earlier routes", which prepends).
 *
 * Browsers with CSS scroll anchoring (`overflow-anchor`) do this on their
 * own, but not all do, and their choice of anchor is theirs. So: before
 * the insert, note the first item still on screen and where it is; after
 * it, scroll by however far that item moved. When the browser already
 * kept it in place the distance is 0 and nothing scrolls. */

/** The item to hold still: its `attribute` value and its top, in viewport
 * pixels. */
export interface ScrollAnchor {
  attribute: string;
  value: string;
  top: number;
}

/** The first `[attribute]` element in `container` whose bottom is below the
 * top of the viewport (so at least partly on screen, or below it), or the
 * first one at all; `null` when there is none. */
export function captureScrollAnchor(container: ParentNode | null, attribute: string): ScrollAnchor | null {
  if (!container) return null;
  const items = Array.from(container.querySelectorAll<HTMLElement>(`[${attribute}]`));
  const visible = items.find((item) => item.getBoundingClientRect().bottom > 0) ?? items[0];
  const value = visible?.getAttribute(attribute);
  if (!visible || value == null) return null;
  return { attribute, value, top: visible.getBoundingClientRect().top };
}

/** Scrolls the window so `anchor`'s element is back where it was. Returns
 * the distance scrolled (0 when it hadn't moved or is gone). */
export function restoreScrollAnchor(container: ParentNode | null, anchor: ScrollAnchor | null): number {
  if (!container || !anchor) return 0;
  const item = Array.from(container.querySelectorAll<HTMLElement>(`[${anchor.attribute}]`)).find(
    (element) => element.getAttribute(anchor.attribute) === anchor.value,
  );
  if (!item) return 0;
  const moved = item.getBoundingClientRect().top - anchor.top;
  if (moved !== 0) window.scrollBy(0, moved);
  return moved;
}
