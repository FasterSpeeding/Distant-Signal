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
