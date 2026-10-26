/** The `text-transform` a Mantine `Badge` renders its label with, read from
 * the root's `tt` style prop. `''` means Mantine's default (uppercase, from
 * Badge.css); `'none'` means the label shows as written, which
 * docs/style-guide.md asks for on any badge whose label is a phrase or
 * carries units ("12m late", "Live departure board"). */
export function badgeTextTransform(labelEl: HTMLElement): string {
  const root = labelEl.closest<HTMLElement>('.mantine-Badge-root');
  if (!root) throw new Error('not inside a Mantine Badge');
  return root.style.textTransform;
}

/** The text a screen reader gets from `el`: its text content without
 * anything under `aria-hidden="true"` (a badge's phone form, a drawing
 * the badge says in words instead). */
export function spokenText(el: Element | null): string {
  if (el === null) return '';
  let text = '';
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) text += node.textContent ?? '';
    else if (node instanceof Element && node.getAttribute('aria-hidden') !== 'true') text += spokenText(node);
  }
  return text;
}
